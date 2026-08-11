use std::path::Path;
use std::time::Duration;

use reqwest::multipart::{Form, Part};
use serde_json::{Map, Value, json};
use uuid::Uuid;

use crate::installer;
use crate::models::{AppSettings, GenerationJob, GenerationRequest};
use crate::runtime;
use crate::state::AppState;

#[cfg(test)]
const FL2VA: &str = "minimax_h3_fl2va_pruned_int8_convrot.safetensors";
#[cfg(test)]
const TEXT_ENCODER: &str = "qwen3vl_32b_minimax_h3_nvfp4_awq.safetensors";
#[cfg(test)]
const VIDEO_VAE: &str = "minimax_h3_video_vae_fp16.safetensors";
#[cfg(test)]
const AUDIO_VAE: &str = "minimax_h3_audio_vae_fp32.safetensors";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum WorkflowModelKind {
    Fl2va,
    Ref2va,
}

struct WorkflowModels {
    diffusion_model: String,
    kind: WorkflowModelKind,
    text_encoder: String,
    video_vae: String,
    audio_vae: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ReferenceMediaKind {
    Image,
    Video,
    Audio,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct UploadedReference {
    name: String,
    kind: ReferenceMediaKind,
}

pub async fn queue(
    request: GenerationRequest,
    settings: &AppSettings,
    state: &AppState,
) -> Result<GenerationJob, String> {
    validate_generation(&request)?;
    let comfy_url = runtime::active_url(state)
        .await
        .ok_or_else(|| "本地 ComfyUI 尚未就绪，请先启动本地引擎。".to_string())?;
    let client = http_client()?;
    let models = workflow_models(
        Path::new(&settings.model_path),
        request.selected_model.as_deref(),
        settings.enabled_models.as_deref(),
    )?;
    let has_reference = request.first_frame_path.is_some()
        || request.last_frame_path.is_some()
        || !request.reference_paths.is_empty();
    if models.kind == WorkflowModelKind::Ref2va && !has_reference {
        return Err("MiniMax H3 Ref2VA 需要至少添加一项图片、视频或音频参考素材。".into());
    }
    if models.kind == WorkflowModelKind::Fl2va && !request.reference_paths.is_empty() {
        return Err("全模态参考素材需要选择 MiniMax H3 Ref2VA 模型。".into());
    }
    let uploaded_first = match request.first_frame_path.as_deref() {
        Some(path) => Some(upload_input(&client, &comfy_url, Path::new(path)).await?),
        None => None,
    };
    let uploaded_last = match request.last_frame_path.as_deref() {
        Some(path) => Some(upload_input(&client, &comfy_url, Path::new(path)).await?),
        None => None,
    };
    let mut uploaded_references = Vec::with_capacity(request.reference_paths.len());
    for path in &request.reference_paths {
        let path = Path::new(path);
        uploaded_references.push(UploadedReference {
            name: upload_input(&client, &comfy_url, path).await?,
            kind: reference_media_kind(path)?,
        });
    }
    let client_id = Uuid::new_v4().to_string();
    let workflow = build_workflow(
        &request,
        uploaded_first.as_deref(),
        uploaded_last.as_deref(),
        &uploaded_references,
        &models,
    );
    let response = client
        .post(format!("{comfy_url}/prompt"))
        .json(&json!({ "prompt": workflow, "client_id": client_id }))
        .send()
        .await
        .map_err(|error| format!("提交生成任务失败：{error}"))?;
    let status = response.status();
    let body: Value = response
        .json()
        .await
        .map_err(|error| format!("ComfyUI 返回了无法解析的响应：{error}"))?;
    if !status.is_success() {
        let details = body
            .get("error")
            .and_then(Value::as_object)
            .and_then(|error| error.get("message"))
            .and_then(Value::as_str)
            .or_else(|| body.get("message").and_then(Value::as_str))
            .unwrap_or("未知工作流错误");
        return Err(format!("ComfyUI 拒绝了工作流：{details}"));
    }
    let prompt_id = body
        .get("prompt_id")
        .and_then(Value::as_str)
        .ok_or_else(|| "ComfyUI 没有返回任务编号。".to_string())?
        .to_string();
    Ok(GenerationJob {
        prompt_id,
        client_id,
        state: "queued".into(),
        message: "任务已进入本地生成队列。".into(),
        video_url: None,
        local_video_path: None,
        filename: None,
    })
}

pub async fn poll(
    prompt_id: &str,
    client_id: &str,
    settings: &AppSettings,
    state: &AppState,
) -> Result<GenerationJob, String> {
    let prompt_uuid = Uuid::parse_str(prompt_id).map_err(|_| "任务编号格式无效。".to_string())?;
    let client_uuid = Uuid::parse_str(client_id).map_err(|_| "客户端编号格式无效。".to_string())?;
    let comfy_url = runtime::active_url(state)
        .await
        .ok_or_else(|| "本地 ComfyUI 已断开，无法读取任务状态。".to_string())?;
    let client = http_client()?;
    let response = client
        .get(format!("{comfy_url}/history/{prompt_uuid}"))
        .send()
        .await
        .map_err(|error| format!("读取任务状态失败：{error}"))?;
    if !response.status().is_success() {
        return Err(format!(
            "读取任务状态失败：ComfyUI 返回 {}。",
            response.status()
        ));
    }
    let history: Value = response
        .json()
        .await
        .map_err(|error| format!("无法解析任务状态：{error}"))?;
    let Some(entry) = history.get(prompt_uuid.to_string()) else {
        return Ok(GenerationJob {
            prompt_id: prompt_uuid.to_string(),
            client_id: client_uuid.to_string(),
            state: "running".into(),
            message: "正在加载模型并生成视频。首次运行通常需要更久。".into(),
            video_url: None,
            local_video_path: None,
            filename: None,
        });
    };

    if let Some((filename, subfolder, file_type)) =
        find_video(entry.get("outputs").unwrap_or(&Value::Null))
    {
        let mut url =
            reqwest::Url::parse(&format!("{comfy_url}/view")).map_err(|error| error.to_string())?;
        url.query_pairs_mut()
            .append_pair("filename", &filename)
            .append_pair("subfolder", &subfolder)
            .append_pair("type", &file_type);
        let local_video_path =
            resolve_local_video_path(&settings.output_path, &filename, &subfolder, &file_type);
        return Ok(GenerationJob {
            prompt_id: prompt_uuid.to_string(),
            client_id: client_uuid.to_string(),
            state: "completed".into(),
            message: "视频与声音已经生成完成。".into(),
            video_url: Some(url.to_string()),
            local_video_path,
            filename: Some(filename),
        });
    }

    let completed = entry
        .pointer("/status/completed")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let failed_message = extract_execution_error(entry);
    if failed_message.is_some() || completed {
        return Ok(GenerationJob {
            prompt_id: prompt_uuid.to_string(),
            client_id: client_uuid.to_string(),
            state: "failed".into(),
            message: failed_message
                .unwrap_or_else(|| "任务已结束，但没有找到输出视频。请查看 ComfyUI 日志。".into()),
            video_url: None,
            local_video_path: None,
            filename: None,
        });
    }

    Ok(GenerationJob {
        prompt_id: prompt_uuid.to_string(),
        client_id: client_uuid.to_string(),
        state: "running".into(),
        message: "H3 正在共同生成画面与双声道音频。".into(),
        video_url: None,
        local_video_path: None,
        filename: None,
    })
}

fn resolve_local_video_path(
    output_root: &str,
    filename: &str,
    subfolder: &str,
    file_type: &str,
) -> Option<String> {
    let filename_path = Path::new(filename);
    let subfolder_path = Path::new(subfolder);
    if file_type != "output" || filename_path.is_absolute() || subfolder_path.is_absolute() {
        return None;
    }
    let extension = filename_path
        .extension()
        .and_then(|value| value.to_str())?
        .to_ascii_lowercase();
    if !matches!(extension.as_str(), "mp4" | "webm" | "mkv" | "mov") {
        return None;
    }
    let root = Path::new(output_root).canonicalize().ok()?;
    let nested_candidate = root.join(subfolder_path).join(filename_path);
    let direct_candidate = root.join(filename_path);
    [nested_candidate, direct_candidate]
        .into_iter()
        .filter_map(|candidate| candidate.canonicalize().ok())
        .find(|candidate| candidate.starts_with(&root) && candidate.is_file())
        .map(|candidate| frontend_file_path(&candidate))
}

fn frontend_file_path(path: &Path) -> String {
    let value = path.to_string_lossy();
    if let Some(network_path) = value.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{network_path}")
    } else {
        value.strip_prefix(r"\\?\").unwrap_or(&value).to_string()
    }
}

pub async fn cancel(state: &AppState) -> Result<(), String> {
    let comfy_url = runtime::active_url(state)
        .await
        .ok_or_else(|| "本地 ComfyUI 尚未运行。".to_string())?;
    let client = http_client()?;
    let response = client
        .post(format!("{comfy_url}/interrupt"))
        .send()
        .await
        .map_err(|error| format!("取消任务失败：{error}"))?;
    if !response.status().is_success() {
        return Err(format!(
            "取消任务失败：ComfyUI 返回 {}。",
            response.status()
        ));
    }
    Ok(())
}

fn validate_generation(request: &GenerationRequest) -> Result<(), String> {
    let prompt = request.prompt.trim();
    if prompt.is_empty() || prompt.chars().count() > 5000 {
        return Err("创作描述必须包含 1–5000 个字符。".into());
    }
    if request.width < 320
        || request.height < 320
        || request.width > 2048
        || request.height > 2048
        || !request.width.is_multiple_of(32)
        || !request.height.is_multiple_of(32)
    {
        return Err("宽高必须在 320–2048 之间，并且是 32 的倍数。".into());
    }
    if !(4.0..=15.0).contains(&request.duration_seconds) {
        return Err("H3 视频时长必须在 4–15 秒之间。".into());
    }
    if !(12..=60).contains(&request.fps) {
        return Err("视频帧率必须在 12–60 FPS 之间。".into());
    }
    if !(8..=40).contains(&request.steps) {
        return Err("采样步数必须在 8–40 之间。".into());
    }
    if let Some(path) = request.first_frame_path.as_deref() {
        validate_reference_path(Path::new(path), "首帧图片", Some(ReferenceMediaKind::Image))?;
    }
    if let Some(path) = request.last_frame_path.as_deref() {
        validate_reference_path(Path::new(path), "尾帧图片", Some(ReferenceMediaKind::Image))?;
    }
    let mut counts = [0usize; 3];
    for path in &request.reference_paths {
        let path = Path::new(path);
        let kind = reference_media_kind(path)?;
        validate_reference_path(path, "全模态参考素材", None)?;
        counts[match kind {
            ReferenceMediaKind::Image => 0,
            ReferenceMediaKind::Video => 1,
            ReferenceMediaKind::Audio => 2,
        }] += 1;
    }
    if counts[0] > 9 || counts[1] > 3 || counts[2] > 3 {
        return Err("Ref2VA 最多支持 9 张参考图片、3 个参考视频和 3 个参考音频。".into());
    }
    Ok(())
}

fn validate_reference_path(
    path: &Path,
    label: &str,
    required_kind: Option<ReferenceMediaKind>,
) -> Result<(), String> {
    if !path.is_absolute() || !path.is_file() {
        return Err(format!("{label}不存在或路径无效。"));
    }
    let kind = reference_media_kind(path)?;
    if required_kind.is_some_and(|required| required != kind) {
        return Err(format!("{label}必须是 PNG、JPG、JPEG 或 WebP 图片。"));
    }
    let size = std::fs::metadata(path)
        .map_err(|error| format!("无法读取{label}：{error}"))?
        .len();
    let limit = match kind {
        ReferenceMediaKind::Image => 50 * 1024 * 1024,
        ReferenceMediaKind::Video => 2 * 1024 * 1024 * 1024,
        ReferenceMediaKind::Audio => 500 * 1024 * 1024,
    };
    if size > limit {
        return Err(format!(
            "{label}文件过大。图片不超过 50 MB，视频不超过 2 GB，音频不超过 500 MB。"
        ));
    }
    Ok(())
}

fn reference_media_kind(path: &Path) -> Result<ReferenceMediaKind, String> {
    let extension = path
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    match extension.as_str() {
        "png" | "jpg" | "jpeg" | "webp" => Ok(ReferenceMediaKind::Image),
        "mp4" | "mov" | "mkv" | "webm" | "avi" => Ok(ReferenceMediaKind::Video),
        "wav" | "mp3" | "flac" | "m4a" | "ogg" | "aac" => Ok(ReferenceMediaKind::Audio),
        _ => Err("参考素材格式不受支持。请选择图片、MP4/MOV/MKV/WebM/AVI 视频，或 WAV/MP3/FLAC/M4A/OGG/AAC 音频。".into()),
    }
}

async fn upload_input(
    client: &reqwest::Client,
    comfy_url: &str,
    path: &Path,
) -> Result<String, String> {
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| "参考素材文件名无效。".to_string())?;
    let extension = path
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let mime = match extension.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        "mp4" => "video/mp4",
        "mov" => "video/quicktime",
        "mkv" => "video/x-matroska",
        "webm" => "video/webm",
        "avi" => "video/x-msvideo",
        "wav" => "audio/wav",
        "mp3" => "audio/mpeg",
        "flac" => "audio/flac",
        "m4a" => "audio/mp4",
        "ogg" => "audio/ogg",
        "aac" => "audio/aac",
        _ => return Err("参考素材格式不受支持。".into()),
    };
    let part = Part::file(path)
        .await
        .map_err(|error| format!("无法读取参考素材：{error}"))?
        .file_name(file_name.to_string())
        .mime_str(mime)
        .map_err(|error| format!("无法准备参考素材：{error}"))?;
    let form = Form::new()
        .part("image", part)
        .text("type", "input")
        .text("overwrite", "true");
    let response = client
        .post(format!("{comfy_url}/upload/image"))
        .multipart(form)
        .send()
        .await
        .map_err(|error| format!("上传参考素材到本地 ComfyUI 失败：{error}"))?;
    if !response.status().is_success() {
        return Err(format!("ComfyUI 无法接收参考素材：{}。", response.status()));
    }
    let body: Value = response
        .json()
        .await
        .map_err(|error| format!("无法解析参考素材上传结果：{error}"))?;
    body.get("name")
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| "ComfyUI 没有返回已上传参考素材的文件名。".to_string())
}

fn workflow_models(
    model_root: &Path,
    selected_model: Option<&str>,
    enabled_models: Option<&[String]>,
) -> Result<WorkflowModels, String> {
    let fl2va = installer::active_model_filename(model_root, "fl2va");
    let ref2va = installer::active_model_filename(model_root, "ref2va");
    let requested_name = selected_model
        .filter(|value| !value.trim().is_empty())
        .and_then(|value| Path::new(value).file_name())
        .and_then(|name| name.to_str());
    let (diffusion_model, kind) = match requested_name {
        Some(requested)
            if fl2va
                .as_deref()
                .is_some_and(|name| name.eq_ignore_ascii_case(requested)) =>
        {
            (requested.to_string(), WorkflowModelKind::Fl2va)
        }
        Some(requested)
            if ref2va
                .as_deref()
                .is_some_and(|name| name.eq_ignore_ascii_case(requested)) =>
        {
            (requested.to_string(), WorkflowModelKind::Ref2va)
        }
        Some(_) => {
            return Err("所选模型不是当前模型目录中可用的 MiniMax H3 生成模型。".into());
        }
        None => (
            fl2va.ok_or_else(|| "未找到可用的 MiniMax H3 FL2VA 主模型。".to_string())?,
            WorkflowModelKind::Fl2va,
        ),
    };
    if !model_enabled(enabled_models, &diffusion_model) {
        return Err("所选生成模型已在模型管理中停用。".into());
    }

    let text_encoder = installer::active_model_filename(model_root, "text_encoder")
        .ok_or_else(|| "未找到 MiniMax H3 文本编码器。".to_string())?;
    let video_vae = installer::active_model_filename(model_root, "video_vae")
        .ok_or_else(|| "未找到 MiniMax H3 视频 VAE。".to_string())?;
    let audio_vae = installer::active_model_filename(model_root, "audio_vae")
        .ok_or_else(|| "未找到 MiniMax H3 音频 VAE。".to_string())?;
    for (label, filename) in [
        ("文本编码器", &text_encoder),
        ("视频 VAE", &video_vae),
        ("音频 VAE", &audio_vae),
    ] {
        if !model_enabled(enabled_models, filename) {
            return Err(format!("{label}已在模型管理中停用，请启用后再生成。"));
        }
    }

    Ok(WorkflowModels {
        diffusion_model,
        kind,
        text_encoder,
        video_vae,
        audio_vae,
    })
}

fn model_enabled(enabled_models: Option<&[String]>, filename: &str) -> bool {
    enabled_models.is_none_or(|models| {
        models.iter().any(|path| {
            Path::new(path)
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.eq_ignore_ascii_case(filename))
        })
    })
}

fn build_workflow(
    request: &GenerationRequest,
    uploaded_first: Option<&str>,
    uploaded_last: Option<&str>,
    uploaded_references: &[UploadedReference],
    models: &WorkflowModels,
) -> Value {
    let raw_frames = (request.duration_seconds * request.fps as f64)
        .round()
        .max(5.0) as u32;
    let length = raw_frames + (5 + 17 - raw_frames % 17) % 17;
    let prefix: String = request
        .output_prefix
        .chars()
        .filter(|character| character.is_ascii_alphanumeric() || matches!(character, '_' | '-'))
        .take(40)
        .collect();
    let prefix = if prefix.is_empty() {
        "MagineStudio"
    } else {
        &prefix
    };
    let noise_seed = if request.seed < 0 {
        (Uuid::new_v4().as_u128() & u64::MAX as u128) as u64
    } else {
        request.seed as u64
    };

    let mut nodes = Map::new();
    nodes.insert(
        "1".into(),
        node(
            "UNETLoader",
            json!({ "unet_name": models.diffusion_model, "weight_dtype": "default" }),
        ),
    );
    nodes.insert(
        "2".into(),
        node(
            "CLIPLoader",
            json!({ "clip_name": models.text_encoder, "type": "minimax", "device": "default" }),
        ),
    );
    nodes.insert(
        "3".into(),
        node("VAELoader", json!({ "vae_name": models.video_vae })),
    );
    nodes.insert(
        "4".into(),
        node("VAELoader", json!({ "vae_name": models.audio_vae })),
    );
    let mut next_node_id = 15u32;
    let mut add_input_node = |nodes: &mut Map<String, Value>, class_type: &str, inputs: Value| {
        let id = next_node_id.to_string();
        next_node_id += 1;
        nodes.insert(id.clone(), node(class_type, inputs));
        id
    };
    let first_frame_node = uploaded_first
        .map(|name| add_input_node(&mut nodes, "LoadImage", json!({ "image": name })));
    let last_frame_node =
        uploaded_last.map(|name| add_input_node(&mut nodes, "LoadImage", json!({ "image": name })));
    let mut reference_image_nodes = Vec::new();
    let mut reference_video_nodes = Vec::new();
    let mut reference_audio_nodes = Vec::new();
    if models.kind == WorkflowModelKind::Ref2va {
        reference_image_nodes.extend(first_frame_node.iter().cloned());
        reference_image_nodes.extend(last_frame_node.iter().cloned());
    }
    for reference in uploaded_references {
        match reference.kind {
            ReferenceMediaKind::Image => reference_image_nodes.push(add_input_node(
                &mut nodes,
                "LoadImage",
                json!({ "image": reference.name }),
            )),
            ReferenceMediaKind::Video => {
                let video =
                    add_input_node(&mut nodes, "LoadVideo", json!({ "file": reference.name }));
                reference_video_nodes.push(add_input_node(
                    &mut nodes,
                    "GetVideoComponents",
                    json!({ "video": [video, 0] }),
                ));
            }
            ReferenceMediaKind::Audio => reference_audio_nodes.push(add_input_node(
                &mut nodes,
                "LoadAudio",
                json!({ "audio": reference.name }),
            )),
        }
    }
    let (conditioning_node, mut conditioning) = match models.kind {
        WorkflowModelKind::Fl2va => (
            "MiniMaxH3ImageToVideo",
            json!({
                "clip": ["2", 0], "vae": ["3", 0], "prompt": request.prompt,
                "width": request.width, "height": request.height, "length": length
            }),
        ),
        WorkflowModelKind::Ref2va => {
            let prompt = reference_prompt(
                &request.prompt,
                reference_image_nodes.len(),
                reference_video_nodes.len(),
                reference_audio_nodes.len(),
            );
            (
                "MiniMaxH3ReferenceToVideo",
                json!({
                    "clip": ["2", 0], "vae": ["3", 0], "audio_vae": ["4", 0],
                    "prompt": prompt, "width": request.width, "height": request.height,
                    "length": length, "ref_image_size": "match"
                }),
            )
        }
    };
    let conditioning_inputs = conditioning
        .as_object_mut()
        .expect("conditioning inputs must be an object");
    match models.kind {
        WorkflowModelKind::Fl2va => {
            if let Some(node_id) = first_frame_node {
                conditioning_inputs.insert("first_frame".into(), json!([node_id, 0]));
            }
            if let Some(node_id) = last_frame_node {
                conditioning_inputs.insert("last_frame".into(), json!([node_id, 0]));
            }
        }
        WorkflowModelKind::Ref2va => {
            for (index, node_id) in reference_image_nodes.iter().enumerate() {
                conditioning_inputs.insert(format!("ref_image_{index}"), json!([node_id, 0]));
            }
            for (index, node_id) in reference_video_nodes.iter().enumerate() {
                conditioning_inputs.insert(format!("ref_video_{index}"), json!([node_id, 0]));
                conditioning_inputs.insert(format!("ref_video_audio_{index}"), json!([node_id, 1]));
            }
            for (index, node_id) in reference_audio_nodes.iter().enumerate() {
                conditioning_inputs.insert(format!("ref_audio_{index}"), json!([node_id, 0]));
            }
        }
    }
    nodes.insert("5".into(), node(conditioning_node, conditioning));
    nodes.insert(
        "6".into(),
        node("RandomNoise", json!({ "noise_seed": noise_seed })),
    );
    nodes.insert(
        "7".into(),
        node("KSamplerSelect", json!({ "sampler_name": "res_multistep" })),
    );
    nodes.insert("8".into(), node("BasicScheduler", json!({ "model": ["1", 0], "scheduler": "simple", "steps": request.steps, "denoise": 1.0 })));
    nodes.insert(
        "9".into(),
        node(
            "BasicGuider",
            json!({ "model": ["1", 0], "conditioning": ["5", 0] }),
        ),
    );
    nodes.insert("10".into(), node("SamplerCustomAdvanced", json!({ "noise": ["6", 0], "guider": ["9", 0], "sampler": ["7", 0], "sigmas": ["8", 0], "latent_image": ["5", 1] })));
    nodes.insert(
        "11".into(),
        node(
            "VAEDecode",
            json!({ "samples": ["10", 0], "vae": ["3", 0] }),
        ),
    );
    nodes.insert(
        "12".into(),
        node(
            "VAEDecodeAudio",
            json!({ "samples": ["10", 0], "vae": ["4", 0] }),
        ),
    );
    nodes.insert(
        "13".into(),
        node(
            "CreateVideo",
            json!({ "images": ["11", 0], "audio": ["12", 0], "fps": request.fps as f64, "bit_depth": 8 }),
        ),
    );
    nodes.insert("14".into(), node("SaveVideo", json!({ "video": ["13", 0], "filename_prefix": format!("video/{prefix}"), "format": "auto", "codec": "auto" })));
    Value::Object(nodes)
}

fn reference_prompt(
    prompt: &str,
    image_count: usize,
    video_count: usize,
    audio_count: usize,
) -> String {
    let mut missing_tags = Vec::new();
    for index in 1..=image_count {
        let tag = format!("<Picture {index}>");
        if !prompt.contains(&tag) {
            missing_tags.push(tag);
        }
    }
    for index in 1..=video_count {
        let tag = format!("<Video {index}>");
        if !prompt.contains(&tag) {
            missing_tags.push(tag);
        }
    }
    for index in 1..=audio_count {
        let tag = format!("<Audio {index}>");
        if !prompt.contains(&tag) {
            missing_tags.push(tag);
        }
    }
    if missing_tags.is_empty() {
        prompt.to_string()
    } else {
        format!("{} {prompt}", missing_tags.join(" "))
    }
}

fn node(class_type: &str, inputs: Value) -> Value {
    json!({ "class_type": class_type, "inputs": inputs, "_meta": { "title": class_type } })
}

fn find_video(value: &Value) -> Option<(String, String, String)> {
    match value {
        Value::Object(map) => {
            if let Some(filename) = map.get("filename").and_then(Value::as_str) {
                let lower = filename.to_ascii_lowercase();
                if lower.ends_with(".mp4")
                    || lower.ends_with(".webm")
                    || lower.ends_with(".mov")
                    || lower.ends_with(".mkv")
                {
                    return Some((
                        filename.to_string(),
                        map.get("subfolder")
                            .and_then(Value::as_str)
                            .unwrap_or("")
                            .to_string(),
                        map.get("type")
                            .and_then(Value::as_str)
                            .unwrap_or("output")
                            .to_string(),
                    ));
                }
            }
            map.values().find_map(find_video)
        }
        Value::Array(values) => values.iter().find_map(find_video),
        _ => None,
    }
}

fn extract_execution_error(entry: &Value) -> Option<String> {
    entry
        .pointer("/status/messages")?
        .as_array()?
        .iter()
        .find_map(|message| {
            let message = message.as_array()?;
            if message.first()?.as_str()? != "execution_error" {
                return None;
            }
            let details = message.get(1)?;
            let exception = details
                .get("exception_message")
                .and_then(Value::as_str)
                .unwrap_or("未知执行错误");
            let node = details
                .get("node_type")
                .and_then(Value::as_str)
                .unwrap_or("工作流节点");
            Some(format!("{node} 执行失败：{exception}"))
        })
}

fn http_client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(30))
        .build()
        .map_err(|error| format!("无法初始化本地连接：{error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request() -> GenerationRequest {
        GenerationRequest {
            prompt: "A cinematic city scene with rain and stereo ambience".into(),
            selected_model: Some(FL2VA.into()),
            width: 960,
            height: 544,
            duration_seconds: 5.0,
            fps: 24,
            steps: 20,
            seed: 42,
            first_frame_path: None,
            last_frame_path: None,
            reference_paths: Vec::new(),
            output_prefix: "Test".into(),
        }
    }

    fn models() -> WorkflowModels {
        WorkflowModels {
            diffusion_model: FL2VA.into(),
            kind: WorkflowModelKind::Fl2va,
            text_encoder: TEXT_ENCODER.into(),
            video_vae: VIDEO_VAE.into(),
            audio_vae: AUDIO_VAE.into(),
        }
    }

    #[test]
    fn workflow_uses_aligned_frame_count() {
        let workflow = build_workflow(&request(), None, None, &[], &models());
        assert_eq!(workflow["5"]["inputs"]["length"], 124);
        assert_eq!(workflow["13"]["inputs"]["fps"], 24.0);
        assert_eq!(workflow["14"]["class_type"], "SaveVideo");
    }

    #[test]
    fn generation_validation_covers_supported_boundaries() {
        let mut valid = request();
        valid.width = 320;
        valid.height = 2048;
        valid.duration_seconds = 4.0;
        valid.fps = 12;
        valid.steps = 8;
        assert!(validate_generation(&valid).is_ok());

        for (label, invalid) in [
            ("empty prompt", {
                let mut value = valid.clone();
                value.prompt = "  ".into();
                value
            }),
            ("unaligned width", {
                let mut value = valid.clone();
                value.width = 321;
                value
            }),
            ("duration too short", {
                let mut value = valid.clone();
                value.duration_seconds = 3.9;
                value
            }),
            ("fps too high", {
                let mut value = valid.clone();
                value.fps = 61;
                value
            }),
            ("steps too low", {
                let mut value = valid.clone();
                value.steps = 7;
                value
            }),
        ] {
            assert!(
                validate_generation(&invalid).is_err(),
                "{label} should be rejected"
            );
        }
    }

    #[test]
    fn selected_frame_rate_changes_frame_count_and_output_rate() {
        let mut request = request();
        request.fps = 30;
        let workflow = build_workflow(&request, None, None, &[], &models());
        assert_eq!(workflow["5"]["inputs"]["length"], 158);
        assert_eq!(workflow["13"]["inputs"]["fps"], 30.0);
    }

    #[test]
    fn negative_seed_is_converted_to_valid_random_seed() {
        let mut request = request();
        request.seed = -1;
        let workflow = build_workflow(&request, None, None, &[], &models());
        assert!(workflow["6"]["inputs"]["noise_seed"].as_u64().is_some());
    }

    #[test]
    fn first_and_last_frames_are_connected_to_fl2va() {
        let workflow = build_workflow(
            &request(),
            Some("first.png"),
            Some("last.png"),
            &[],
            &models(),
        );
        assert_eq!(workflow["5"]["inputs"]["first_frame"][0], "15");
        assert_eq!(workflow["5"]["inputs"]["last_frame"][0], "16");
    }

    #[test]
    fn reference_model_uses_reference_conditioning() {
        let mut models = models();
        models.diffusion_model = "minimax_h3_ref2va_pruned_fp8_scaled.safetensors".into();
        models.kind = WorkflowModelKind::Ref2va;
        let workflow = build_workflow(&request(), Some("reference.png"), None, &[], &models);
        assert_eq!(workflow["5"]["class_type"], "MiniMaxH3ReferenceToVideo");
        assert_eq!(workflow["5"]["inputs"]["ref_image_0"][0], "15");
        assert!(
            workflow["5"]["inputs"]["prompt"]
                .as_str()
                .is_some_and(|prompt| prompt.starts_with("<Picture 1>"))
        );
    }

    #[test]
    fn reference_model_connects_image_video_and_audio_inputs() {
        let mut models = models();
        models.diffusion_model = "minimax_h3_ref2va_pruned_fp8_scaled.safetensors".into();
        models.kind = WorkflowModelKind::Ref2va;
        let references = vec![
            UploadedReference {
                name: "look.png".into(),
                kind: ReferenceMediaKind::Image,
            },
            UploadedReference {
                name: "motion.mp4".into(),
                kind: ReferenceMediaKind::Video,
            },
            UploadedReference {
                name: "voice.wav".into(),
                kind: ReferenceMediaKind::Audio,
            },
        ];
        let workflow = build_workflow(&request(), None, None, &references, &models);
        assert_eq!(workflow["15"]["class_type"], "LoadImage");
        assert_eq!(workflow["16"]["class_type"], "LoadVideo");
        assert_eq!(workflow["17"]["class_type"], "GetVideoComponents");
        assert_eq!(workflow["18"]["class_type"], "LoadAudio");
        assert_eq!(workflow["5"]["inputs"]["ref_image_0"][0], "15");
        assert_eq!(workflow["5"]["inputs"]["ref_video_0"][0], "17");
        assert_eq!(workflow["5"]["inputs"]["ref_video_audio_0"][1], 1);
        assert_eq!(workflow["5"]["inputs"]["ref_audio_0"][0], "18");
        let prompt = workflow["5"]["inputs"]["prompt"].as_str().unwrap();
        assert!(prompt.contains("<Picture 1>"));
        assert!(prompt.contains("<Video 1>"));
        assert!(prompt.contains("<Audio 1>"));
    }

    #[test]
    fn recursive_video_finder_handles_history_shape() {
        let value = json!({ "14": { "videos": [{ "filename": "video/H3_00001_.mp4", "subfolder": "video", "type": "output" }] } });
        let found = find_video(&value).expect("video");
        assert!(found.0.ends_with(".mp4"));
    }

    #[test]
    fn local_video_path_accepts_comfy_subfolder_shapes_and_rejects_escape() {
        let root = std::env::temp_dir().join(format!("magine-preview-{}", Uuid::new_v4()));
        let video_dir = root.join("video");
        std::fs::create_dir_all(&video_dir).unwrap();
        let video = video_dir.join("MagineStudio_00001_.mp4");
        std::fs::write(&video, b"preview").unwrap();
        let canonical_video = video.to_string_lossy().to_string();
        let root_text = root.to_string_lossy();

        let split_shape =
            resolve_local_video_path(&root_text, "MagineStudio_00001_.mp4", "video", "output");
        let combined_shape = resolve_local_video_path(
            &root_text,
            "video/MagineStudio_00001_.mp4",
            "video",
            "output",
        );
        assert_eq!(split_shape.as_deref(), Some(canonical_video.as_str()));
        assert_eq!(combined_shape.as_deref(), Some(canonical_video.as_str()));
        assert!(resolve_local_video_path(&root_text, "../outside.mp4", "", "output").is_none());
        assert!(
            resolve_local_video_path(&root_text, "MagineStudio_00001_.mp4", "video", "temp")
                .is_none()
        );

        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn enabled_model_matching_uses_filename() {
        let enabled = vec![r"G:\MiniMax-H3-Models\unet\model.safetensors".to_string()];
        assert!(model_enabled(Some(&enabled), "model.safetensors"));
        assert!(!model_enabled(Some(&enabled), "other.safetensors"));
        assert!(model_enabled(None, "legacy-default.safetensors"));
    }
}

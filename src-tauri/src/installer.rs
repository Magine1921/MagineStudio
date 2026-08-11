use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use chrono::Utc;
use futures_util::StreamExt;
use reqwest::header::{ACCEPT_ENCODING, RANGE};
use sha2::{Digest, Sha256};
use sysinfo::Disks;
use tauri::{AppHandle, Emitter};
use tokio::fs::{self, File, OpenOptions};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use crate::comfy_api;
use crate::models::{
    AppSettings, EnvironmentComponentStatus, EnvironmentReport, InstallComponent, InstallMarker,
    InstallPlan, InstallProgress, InstallRequest, InstallationStatus, LICENSE_VERSION,
    PathSpaceInfo,
};
use crate::state::AppState;

const GIB: u64 = 1024 * 1024 * 1024;
const COMFY_VERSION: &str = "0.31.0";
const COMFY_ARCHIVE: &str = "ComfyUI_windows_portable_nvidia_cu126.7z";
const COMFY_URL: &str = "https://github.com/Comfy-Org/ComfyUI/releases/download/v0.31.0/ComfyUI_windows_portable_nvidia_cu126.7z";
const COMFY_BYTES: u64 = 2_067_155_290;
const COMFY_SHA256: &str = "5cddeffad0e4e8f7f89fed599d6ff5ea7bfa47cd78c1af4b5a79d00cbfe30050";

const MODEL_BASE_URL: &str = "https://huggingface.co/Comfy-Org/MiniMax-H3/resolve/main";

#[derive(Clone, Copy)]
struct ModelAlternative {
    relative_path: &'static str,
    bytes: u64,
}

#[derive(Clone, Copy)]
struct ModelSpec {
    id: &'static str,
    relative_path: &'static str,
    bytes: u64,
    sha256: &'static str,
    reference_only: bool,
    alternatives: &'static [ModelAlternative],
}

const FL2VA_ALTERNATIVES: &[ModelAlternative] = &[
    ModelAlternative {
        relative_path: "unet/minimax_h3_fl2va_pruned_fp8_scaled.safetensors",
        bytes: 20_958_205_608,
    },
    ModelAlternative {
        relative_path: "diffusion_models/minimax_h3_fl2va_pruned_fp8_scaled.safetensors",
        bytes: 20_958_205_608,
    },
];

const REF2VA_ALTERNATIVES: &[ModelAlternative] = &[
    ModelAlternative {
        relative_path: "unet/minimax_h3_ref2va_pruned_fp8_scaled.safetensors",
        bytes: 20_958_205_608,
    },
    ModelAlternative {
        relative_path: "diffusion_models/minimax_h3_ref2va_pruned_fp8_scaled.safetensors",
        bytes: 20_958_205_608,
    },
];

const MODELS: &[ModelSpec] = &[
    ModelSpec {
        id: "fl2va",
        relative_path: "diffusion_models/minimax_h3_fl2va_pruned_int8_convrot.safetensors",
        bytes: 20_970_379_616,
        sha256: "e889202c41dafb67b10d67b97f0d8541508036a6090af23425a5c2615d03c47a",
        reference_only: false,
        alternatives: FL2VA_ALTERNATIVES,
    },
    ModelSpec {
        id: "text_encoder",
        relative_path: "text_encoders/qwen3vl_32b_minimax_h3_nvfp4_awq.safetensors",
        bytes: 15_687_142_551,
        sha256: "35a88d51044231fe332301d7a62aa81e3f2cba62febeb446e2c1e3e0ef76f2c6",
        reference_only: false,
        alternatives: &[],
    },
    ModelSpec {
        id: "audio_vae",
        relative_path: "vae/minimax_h3_audio_vae_fp32.safetensors",
        bytes: 605_254_808,
        sha256: "8e505d95dd1561d47abd43d4238fd40d9bb1ae9e147ed0a4cba778d76ae4db48",
        reference_only: false,
        alternatives: &[],
    },
    ModelSpec {
        id: "video_vae",
        relative_path: "vae/minimax_h3_video_vae_fp16.safetensors",
        bytes: 5_207_808_496,
        sha256: "7c1f131492e7eddacaac9069a61b81bdd39de5cc96561e677c5eab1cdce5e522",
        reference_only: false,
        alternatives: &[],
    },
    ModelSpec {
        id: "ref2va",
        relative_path: "diffusion_models/minimax_h3_ref2va_pruned_int8_convrot.safetensors",
        bytes: 20_970_379_616,
        sha256: "9255f52b6677845ad238f20dfaafa94727053694127ab7f255c048f0f9365779",
        reference_only: true,
        alternatives: REF2VA_ALTERNATIVES,
    },
];

struct DownloadSpec {
    name: String,
    url: String,
    destination: PathBuf,
    bytes: u64,
    sha256: String,
}

#[derive(Clone, Debug)]
pub struct RuntimeLayout {
    pub root: PathBuf,
    pub python: PathBuf,
    pub main_py: PathBuf,
    pub working_directory: PathBuf,
    pub kind: &'static str,
}

struct ModelConfigurationStatus {
    ready: bool,
    version: Option<String>,
    path: Option<String>,
    message: String,
}

pub async fn environment_report(
    settings: &AppSettings,
    include_reference_model: bool,
) -> EnvironmentReport {
    let disks = Disks::new_with_refreshed_list();
    let configured_comfy_root = PathBuf::from(&settings.comfy_path);
    let configured_model_root = PathBuf::from(&settings.model_path);
    let comfy_root = if locate_runtime_layout(&configured_comfy_root).is_some() {
        configured_comfy_root.clone()
    } else {
        discover_comfy_root(&disks).unwrap_or_else(|| configured_comfy_root.clone())
    };
    let configured_model_count =
        count_ready_models(&configured_model_root, include_reference_model);
    let discovered_model_root = discover_model_root(&disks, include_reference_model);
    let model_root = discovered_model_root
        .filter(|path| count_ready_models(path, include_reference_model) > configured_model_count)
        .unwrap_or_else(|| configured_model_root.clone());
    let suggested_comfy_path = (!same_path(&configured_comfy_root, &comfy_root))
        .then(|| comfy_root.to_string_lossy().to_string());
    let suggested_model_path = (!same_path(&configured_model_root, &model_root))
        .then(|| model_root.to_string_lossy().to_string());
    let alignment_message = match (
        suggested_comfy_path.is_some(),
        suggested_model_path.is_some(),
    ) {
        (true, true) => {
            Some("已发现本机现有的 ComfyUI 与 MiniMax H3 模型，建议采用真实路径。".into())
        }
        (true, false) => Some("已发现本机现有的 ComfyUI，建议采用真实安装路径。".into()),
        (false, true) => Some("已发现本机现有的 MiniMax H3 模型，建议采用真实模型路径。".into()),
        (false, false) => None,
    };
    let runtime = locate_runtime_layout(&comfy_root);
    let selected_models: Vec<&ModelSpec> = MODELS
        .iter()
        .filter(|model| !model.reference_only || include_reference_model)
        .collect();
    let installed_models: Vec<&ModelSpec> = selected_models
        .iter()
        .copied()
        .filter(|model| model_file_ready(&model_root, model))
        .collect();
    let missing_model_bytes: u64 = selected_models
        .iter()
        .filter(|model| !model_file_ready(&model_root, model))
        .map(|model| model.bytes)
        .sum();

    let embedded_python = runtime.as_ref().and_then(|layout| {
        command_version(&layout.python, &["--version"])
            .map(|version| (version, layout.python.to_string_lossy().to_string()))
    });
    let system_python = embedded_python.or_else(|| detect_command("python", &["--version"]));
    let git = detect_command("git", &["--version"]);
    let nvidia = detect_command(
        "nvidia-smi",
        &["--query-gpu=driver_version", "--format=csv,noheader"],
    );
    let configuration = detect_model_configuration(runtime.as_ref(), &model_root).await;
    let comfy_version = runtime
        .as_ref()
        .and_then(read_comfy_version)
        .or_else(|| read_marker(&comfy_root).map(|marker| marker.comfy_version));
    let runtime_dependencies_ready = runtime
        .as_ref()
        .map(runtime_dependencies_ready)
        .unwrap_or(false);

    let mut components = vec![
        component_from_command(
            "python",
            "Python 运行环境",
            system_python,
            "未检测到 Python；安装 ComfyUI 时会配置独立运行环境。",
        ),
        component_from_command(
            "git",
            "Git",
            git,
            "未检测到 Git；便携版运行不受影响，扩展节点更新时可能需要。",
        ),
        component_from_command(
            "cuda",
            "NVIDIA 驱动 / CUDA",
            nvidia,
            "未检测到可用的 NVIDIA 驱动或 nvidia-smi。",
        ),
    ];
    components.push(EnvironmentComponentStatus {
        id: "comfyui".into(),
        name: "ComfyUI".into(),
        state: if runtime.is_none() {
            "missing"
        } else if runtime_dependencies_ready {
            "ready"
        } else {
            "partial"
        }
        .into(),
        version: comfy_version,
        path: runtime
            .as_ref()
            .map(|layout| layout.root.to_string_lossy().to_string()),
        message: if runtime.is_some() && runtime_dependencies_ready {
            "检测到可运行的 ComfyUI 与独立 Python。".into()
        } else if runtime.is_some() {
            "检测到 ComfyUI，但依赖版本不完整；执行检查修复后会按 requirements.txt 补齐。".into()
        } else {
            "所选目录中未找到完整的 ComfyUI 便携环境。".into()
        },
    });
    components.push(EnvironmentComponentStatus {
        id: "models".into(),
        name: "MiniMax H3 模型".into(),
        state: if installed_models.len() == selected_models.len() {
            "ready"
        } else if installed_models.is_empty() {
            "missing"
        } else {
            "partial"
        }
        .into(),
        version: Some(format!(
            "{}/{} 个文件",
            installed_models.len(),
            selected_models.len()
        )),
        path: Some(model_root.to_string_lossy().to_string()),
        message: if installed_models.len() == selected_models.len() {
            "所选创作模式需要的模型文件均已存在且大小匹配。".into()
        } else {
            format!(
                "已识别 {} 个，仍缺少 {} 个模型文件。",
                installed_models.len(),
                selected_models.len().saturating_sub(installed_models.len())
            )
        },
    });
    components.push(EnvironmentComponentStatus {
        id: "configuration".into(),
        name: "模型路径配置".into(),
        state: if configuration.ready {
            "ready"
        } else if runtime.is_some() {
            "partial"
        } else {
            "missing"
        }
        .into(),
        version: configuration.version,
        path: configuration.path,
        message: configuration.message,
    });

    let path_requirements = [
        (
            "comfyui",
            "ComfyUI",
            comfy_root.to_string_lossy().to_string(),
            if runtime.is_some() { 0 } else { 8 * GIB },
        ),
        (
            "models",
            "模型",
            model_root.to_string_lossy().to_string(),
            missing_model_bytes,
        ),
        ("output", "输出", settings.output_path.clone(), GIB),
    ];
    let mut detected = Vec::new();
    let mut required_by_mount: HashMap<String, u64> = HashMap::new();
    for (id, label, path, required) in path_requirements {
        let disk = disk_for_path(Path::new(&path), &disks);
        if let Some(disk) = &disk {
            *required_by_mount
                .entry(disk.mount_point().to_string_lossy().to_ascii_lowercase())
                .or_default() += required;
        }
        detected.push((id, label, path, required, disk));
    }
    let path_spaces = detected
        .into_iter()
        .map(|(id, label, path, required, disk)| match disk {
            Some(disk) => {
                let mount = disk.mount_point().to_string_lossy().to_string();
                let combined_required = required_by_mount
                    .get(&mount.to_ascii_lowercase())
                    .copied()
                    .unwrap_or(required);
                let available = disk.available_space();
                PathSpaceInfo {
                    id: id.into(),
                    label: label.into(),
                    path,
                    mount_point: Some(mount.clone()),
                    total_bytes: Some(disk.total_space()),
                    available_bytes: Some(available),
                    required_bytes: required,
                    sufficient: available >= combined_required,
                    message: format!(
                        "{} 可用 {:.1} GB；该磁盘本次合计需要 {:.1} GB。",
                        mount,
                        available as f64 / GIB as f64,
                        combined_required as f64 / GIB as f64
                    ),
                }
            }
            None => PathSpaceInfo {
                id: id.into(),
                label: label.into(),
                path,
                mount_point: None,
                total_bytes: None,
                available_bytes: None,
                required_bytes: required,
                sufficient: false,
                message: "无法识别该路径所在磁盘，请确认盘符存在且当前可访问。".into(),
            },
        })
        .collect::<Vec<_>>();
    let ready_count = components
        .iter()
        .filter(|component| component.state == "ready")
        .count();
    let total_count = components.len();

    EnvironmentReport {
        scanned_at: Utc::now().to_rfc3339(),
        components,
        path_spaces,
        suggested_comfy_path,
        suggested_model_path,
        alignment_message,
        ready_count,
        total_count,
        summary: if ready_count == total_count {
            "所需软件、环境、模型和路径配置均已就绪。".into()
        } else {
            format!("已就绪 {ready_count}/{total_count} 项；安装器只会补齐缺失内容。")
        },
    }
}

async fn detect_model_configuration(
    runtime: Option<&RuntimeLayout>,
    model_root: &Path,
) -> ModelConfigurationStatus {
    if let Some(api) = comfy_api::detect_model_status().await {
        let port = api.url.rsplit(':').next().unwrap_or_default();
        return if api.ready() {
            ModelConfigurationStatus {
                ready: true,
                version: Some(format!("API · {port}")),
                path: Some(api.url),
                message: "已通过正在运行的 ComfyUI API 验证主模型、文本编码器和两套 VAE。".into(),
            }
        } else {
            ModelConfigurationStatus {
                ready: false,
                version: Some(format!("API {}/{}", api.found, api.total)),
                path: Some(api.url),
                message: format!(
                    "ComfyUI API 当前只能识别 {}/{} 个必要模型，请检查有效模型路径。",
                    api.found, api.total
                ),
            }
        };
    }

    let config_paths = model_config_paths(runtime);
    for path in &config_paths {
        let Ok(content) = std::fs::read_to_string(path) else {
            continue;
        };
        if config_file_registers_models(&content, model_root) {
            let desktop_managed = path
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.eq_ignore_ascii_case("shared_model_paths.yaml"));
            return ModelConfigurationStatus {
                ready: true,
                version: Some(if desktop_managed {
                    "Desktop 共享配置".into()
                } else {
                    "本地路径配置".into()
                }),
                path: Some(path.to_string_lossy().to_string()),
                message: if desktop_managed {
                    "ComfyUI Desktop 共享配置已完整注册当前 MiniMax H3 模型目录。".into()
                } else {
                    "ComfyUI 已完整注册当前 MiniMax H3 模型目录。".into()
                },
            };
        }
    }

    ModelConfigurationStatus {
        ready: false,
        version: None,
        path: config_paths
            .first()
            .map(|path| path.to_string_lossy().to_string()),
        message: if runtime.is_some() {
            "检测到 ComfyUI，但项目配置和 Desktop 共享配置均未完整注册当前模型目录。".into()
        } else {
            "安装 ComfyUI 后将自动写入模型目录配置。".into()
        },
    }
}

fn model_config_paths(runtime: Option<&RuntimeLayout>) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    if let Some(path) = runtime
        .and_then(|layout| layout.main_py.parent())
        .map(|root| root.join("extra_model_paths.yaml"))
    {
        paths.push(path);
    }
    if let Some(app_data) = std::env::var_os("APPDATA") {
        let shared = PathBuf::from(app_data)
            .join("Comfy Desktop")
            .join("shared_model_paths.yaml");
        if !paths.iter().any(|path| same_path(path, &shared)) {
            paths.push(shared);
        }
    }
    paths
}

fn config_file_registers_models(content: &str, model_root: &Path) -> bool {
    let normalized_content = content.replace('\\', "/").to_ascii_lowercase();
    let normalized_root = model_root
        .to_string_lossy()
        .replace('\\', "/")
        .trim_end_matches('/')
        .to_ascii_lowercase();
    if normalized_root.is_empty() || !normalized_content.contains(&normalized_root) {
        return false;
    }

    required_model_categories(model_root)
        .iter()
        .all(|category| has_yaml_key(content, category))
}

fn required_model_categories(model_root: &Path) -> HashSet<String> {
    MODELS
        .iter()
        .filter_map(|model| model_match(model_root, model))
        .filter_map(|relative| relative.split('/').next().map(str::to_string))
        .collect()
}

fn has_yaml_key(content: &str, expected: &str) -> bool {
    content.lines().any(|line| {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            return false;
        }
        line.split_once(':').is_some_and(|(key, _)| {
            key.trim()
                .trim_matches(['\'', '"'])
                .eq_ignore_ascii_case(expected)
        })
    })
}

fn component_from_command(
    id: &str,
    name: &str,
    detected: Option<(String, String)>,
    missing_message: &str,
) -> EnvironmentComponentStatus {
    match detected {
        Some((version, path)) => EnvironmentComponentStatus {
            id: id.into(),
            name: name.into(),
            state: "ready".into(),
            version: Some(version),
            path: Some(path),
            message: "已检测到可用环境。".into(),
        },
        None => EnvironmentComponentStatus {
            id: id.into(),
            name: name.into(),
            state: "missing".into(),
            version: None,
            path: None,
            message: missing_message.into(),
        },
    }
}

fn detect_command(name: &str, args: &[&str]) -> Option<(String, String)> {
    let executable = find_command_path(name)?;
    command_version(&executable, args)
        .map(|version| (version, executable.to_string_lossy().to_string()))
}

fn find_command_path(name: &str) -> Option<PathBuf> {
    #[cfg(windows)]
    let candidates = [format!("{name}.exe"), name.to_string()];
    #[cfg(not(windows))]
    let candidates = [name.to_string(), name.to_string()];
    std::env::var_os("PATH").and_then(|value| {
        std::env::split_paths(&value)
            .flat_map(|directory| {
                candidates
                    .iter()
                    .map(move |candidate| directory.join(candidate))
            })
            .find(|path| path.is_file())
    })
}

fn command_version(executable: &Path, args: &[&str]) -> Option<String> {
    let mut command = Command::new(executable);
    command.args(args);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000);
    }
    let output = command.output().ok()?;
    if !output.status.success() {
        return None;
    }
    let text = if output.stdout.is_empty() {
        String::from_utf8_lossy(&output.stderr)
    } else {
        String::from_utf8_lossy(&output.stdout)
    };
    text.lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(str::to_string)
}

fn read_marker(comfy_root: &Path) -> Option<InstallMarker> {
    let bytes = std::fs::read(comfy_root.join(".h3studio-install.json")).ok()?;
    serde_json::from_slice(&bytes).ok()
}

fn exact_file_ready(path: &Path, bytes: u64) -> bool {
    std::fs::metadata(path)
        .map(|metadata| metadata.is_file() && metadata.len() == bytes)
        .unwrap_or(false)
}

fn relative_destination(model_root: &Path, relative_path: &str) -> PathBuf {
    model_root.join(relative_path.replace('/', std::path::MAIN_SEPARATOR_STR))
}

fn model_match(model_root: &Path, model: &ModelSpec) -> Option<String> {
    if exact_file_ready(&model_destination(model_root, model), model.bytes) {
        return Some(model.relative_path.to_string());
    }
    model.alternatives.iter().find_map(|alternative| {
        exact_file_ready(
            &relative_destination(model_root, alternative.relative_path),
            alternative.bytes,
        )
        .then(|| alternative.relative_path.to_string())
    })
}

fn model_file_ready(model_root: &Path, model: &ModelSpec) -> bool {
    model_match(model_root, model).is_some()
}

fn model_destination(model_root: &Path, model: &ModelSpec) -> PathBuf {
    relative_destination(model_root, model.relative_path)
}

pub fn active_model_filename(model_root: &Path, id: &str) -> Option<String> {
    let model = MODELS.iter().find(|model| model.id == id)?;
    let relative = model_match(model_root, model)?;
    Path::new(&relative)
        .file_name()
        .and_then(|name| name.to_str())
        .map(str::to_string)
}

fn disk_for_path<'a>(path: &Path, disks: &'a Disks) -> Option<&'a sysinfo::Disk> {
    let target = path.to_string_lossy().to_ascii_lowercase();
    disks
        .iter()
        .filter_map(|disk| {
            let mount = disk.mount_point().to_string_lossy().to_ascii_lowercase();
            target.starts_with(&mount).then_some((mount.len(), disk))
        })
        .max_by_key(|(length, _)| *length)
        .map(|(_, disk)| disk)
}

pub fn plan(request: &InstallRequest) -> Result<InstallPlan, String> {
    validate_request_paths(request)?;
    let comfy_root = PathBuf::from(&request.comfy_path);
    let model_root = PathBuf::from(&request.model_path);
    let selected_models: Vec<&ModelSpec> = MODELS
        .iter()
        .filter(|model| !model.reference_only || request.include_reference_model)
        .collect();
    let runtime_layout = locate_runtime_layout(&comfy_root);
    let runtime_ready = runtime_layout.is_some();
    let runtime_healthy = runtime_layout
        .as_ref()
        .map(runtime_dependencies_ready)
        .unwrap_or(false);
    let missing_models: Vec<&&ModelSpec> = selected_models
        .iter()
        .filter(|model| !model_file_ready(&model_root, model))
        .collect();
    let model_bytes: u64 = missing_models.iter().map(|model| model.bytes).sum();
    let download_bytes = if runtime_ready { 0 } else { COMFY_BYTES } + model_bytes;
    let required_free_bytes = if runtime_ready { 0 } else { 8 * GIB } + model_bytes;

    let warnings = Vec::new();

    Ok(InstallPlan {
        components: vec![
            InstallComponent {
                id: "comfyui".into(),
                name: "ComfyUI Windows Portable".into(),
                description: if runtime_healthy {
                    "已检测到完整环境，将复用现有安装".into()
                } else if runtime_ready {
                    "已检测到现有 ComfyUI，将按其 requirements.txt 修复依赖".into()
                } else {
                    "包含隔离的 Python 与 CUDA 12.6 PyTorch 运行时".into()
                },
                bytes: COMFY_BYTES,
                required: true,
                selected: !runtime_healthy,
            },
            InstallComponent {
                id: "base".into(),
                name: "H3 基础创作包".into(),
                description: if missing_models.iter().any(|model| !model.reference_only) {
                    "文生视频、首帧生视频、视频与双声道音频 VAE".into()
                } else {
                    "基础模型文件已存在，将直接复用".into()
                },
                bytes: MODELS
                    .iter()
                    .filter(|m| !m.reference_only)
                    .map(|m| m.bytes)
                    .sum(),
                required: true,
                selected: missing_models.iter().any(|model| !model.reference_only),
            },
            InstallComponent {
                id: "reference".into(),
                name: "H3 多参考扩展包".into(),
                description: "参考图片、视频、动作和声音".into(),
                bytes: MODELS
                    .iter()
                    .filter(|m| m.reference_only)
                    .map(|m| m.bytes)
                    .sum(),
                required: false,
                selected: request.include_reference_model
                    && missing_models.iter().any(|model| model.reference_only),
            },
        ],
        download_bytes,
        required_free_bytes,
        warnings,
        model_files: selected_models
            .iter()
            .map(|model| {
                model_match(&model_root, model).unwrap_or_else(|| model.relative_path.to_string())
            })
            .collect(),
        comfy_version: COMFY_VERSION.into(),
        license_version: LICENSE_VERSION.into(),
    })
}

pub async fn install(
    app: &AppHandle,
    state: &AppState,
    request: InstallRequest,
) -> Result<InstallationStatus, String> {
    if state
        .install_running
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return Err("已有安装任务正在运行。".into());
    }
    state.install_cancel.store(false, Ordering::SeqCst);
    let _guard = InstallGuard { state };

    if !request.license_accepted
        || !request.region_confirmed
        || request.license_version != LICENSE_VERSION
    {
        return Err("必须接受当前版本的 MiniMax H3 社区许可并确认所在地区后才能下载模型。".into());
    }
    validate_request_paths(&request)?;
    preflight_space(&request).await?;

    let comfy_root = PathBuf::from(&request.comfy_path);
    let model_root = PathBuf::from(&request.model_path);
    let output_root = PathBuf::from(&request.output_path);
    fs::create_dir_all(&comfy_root)
        .await
        .map_err(io_error("无法创建 ComfyUI 目录"))?;
    fs::create_dir_all(&model_root)
        .await
        .map_err(io_error("无法创建模型目录"))?;
    fs::create_dir_all(&output_root)
        .await
        .map_err(io_error("无法创建输出目录"))?;

    let cache_root = comfy_root.join(".h3studio-cache");
    fs::create_dir_all(&cache_root)
        .await
        .map_err(io_error("无法创建下载缓存"))?;

    let existing_runtime = locate_runtime_layout(&comfy_root);
    let mut downloads = Vec::new();
    if existing_runtime.is_none() {
        downloads.push(DownloadSpec {
            name: COMFY_ARCHIVE.into(),
            url: COMFY_URL.into(),
            destination: cache_root.join(COMFY_ARCHIVE),
            bytes: COMFY_BYTES,
            sha256: COMFY_SHA256.into(),
        });
    }
    for model in MODELS
        .iter()
        .filter(|model| !model.reference_only || request.include_reference_model)
        .filter(|model| !model_file_ready(&model_root, model))
    {
        downloads.push(DownloadSpec {
            name: model
                .relative_path
                .rsplit('/')
                .next()
                .unwrap_or(model.relative_path)
                .into(),
            url: format!("{MODEL_BASE_URL}/{}", model.relative_path),
            destination: model_root.join(
                model
                    .relative_path
                    .replace('/', std::path::MAIN_SEPARATOR_STR),
            ),
            bytes: model.bytes,
            sha256: model.sha256.into(),
        });
    }

    let total_bytes: u64 = downloads.iter().map(|download| download.bytes).sum();
    let client = reqwest::Client::builder()
        .user_agent("MagineStudio/1.0.0")
        .redirect(reqwest::redirect::Policy::limited(10))
        .connect_timeout(Duration::from_secs(30))
        .build()
        .map_err(|error| format!("无法初始化下载器：{error}"))?;

    let mut completed_bytes = 0_u64;
    for download in &downloads {
        check_cancel(state)?;
        let skipped =
            download_file(app, state, &client, download, completed_bytes, total_bytes).await?;
        completed_bytes += download.bytes;
        if skipped {
            emit_progress(
                app,
                InstallProgress {
                    stage: "download".into(),
                    stage_label: "检查现有文件".into(),
                    file_name: Some(download.name.clone()),
                    file_downloaded_bytes: download.bytes,
                    file_total_bytes: download.bytes,
                    total_downloaded_bytes: completed_bytes,
                    total_bytes,
                    percent: percent(completed_bytes, total_bytes),
                    message: "文件已存在且校验通过，已跳过下载。".into(),
                    can_cancel: true,
                },
            );
        }
    }

    check_cancel(state)?;
    emit_progress(
        app,
        InstallProgress {
            stage: "extract".into(),
            stage_label: if existing_runtime.is_some() {
                "复用现有 ComfyUI"
            } else {
                "配置 ComfyUI"
            }
            .into(),
            file_name: existing_runtime.is_none().then(|| COMFY_ARCHIVE.into()),
            file_downloaded_bytes: if existing_runtime.is_some() {
                0
            } else {
                COMFY_BYTES
            },
            file_total_bytes: if existing_runtime.is_some() {
                0
            } else {
                COMFY_BYTES
            },
            total_downloaded_bytes: total_bytes,
            total_bytes,
            percent: 94.0,
            message: if existing_runtime.is_some() {
                "已检测到完整的 ComfyUI 环境，正在写入模型路径配置。".into()
            } else {
                "正在解压隔离运行环境，这通常需要几分钟。".into()
            },
            can_cancel: false,
        },
    );
    if existing_runtime.is_none() {
        extract_comfy(&cache_root.join(COMFY_ARCHIVE), &comfy_root).await?;
    }

    let runtime_layout = locate_runtime_layout(&comfy_root).ok_or_else(|| {
        "ComfyUI 准备完成，但没有找到可配对的 main.py 与独立 Python。请检查安装目录。".to_string()
    })?;
    if !runtime_dependencies_ready(&runtime_layout) {
        repair_runtime_dependencies(app, &runtime_layout, total_bytes).await?;
    }
    configure_model_paths(&runtime_layout, &model_root).await?;
    let runtime_root = runtime_layout.root.clone();

    let selected_files: Vec<String> = MODELS
        .iter()
        .filter(|model| !model.reference_only || request.include_reference_model)
        .filter_map(|model| model_match(&model_root, model))
        .collect();
    let marker = InstallMarker {
        schema_version: 1,
        installed_at: Utc::now().to_rfc3339(),
        comfy_version: COMFY_VERSION.into(),
        license_version: LICENSE_VERSION.into(),
        runtime_path: runtime_root.to_string_lossy().to_string(),
        model_path: model_root.to_string_lossy().to_string(),
        model_files: selected_files.clone(),
    };
    let marker_json =
        serde_json::to_vec_pretty(&marker).map_err(|error| format!("无法生成安装标记：{error}"))?;
    fs::write(comfy_root.join(".h3studio-install.json"), marker_json)
        .await
        .map_err(io_error("无法保存安装标记"))?;

    emit_progress(
        app,
        InstallProgress {
            stage: "complete".into(),
            stage_label: "安装完成".into(),
            file_name: None,
            file_downloaded_bytes: 0,
            file_total_bytes: 0,
            total_downloaded_bytes: total_bytes,
            total_bytes,
            percent: 100.0,
            message: "ComfyUI、模型目录和输出目录已经配置完成。".into(),
            can_cancel: false,
        },
    );

    Ok(InstallationStatus {
        installed: true,
        healthy: runtime_dependencies_ready(&runtime_layout),
        comfy_version: read_comfy_version(&runtime_layout).or_else(|| Some(COMFY_VERSION.into())),
        installed_models: selected_files,
        message: "H3 本地生成环境已准备好。".into(),
        runtime_path: Some(runtime_root.to_string_lossy().to_string()),
    })
}

pub async fn status(settings: &AppSettings) -> InstallationStatus {
    let disks = Disks::new_with_refreshed_list();
    let configured_comfy_root = PathBuf::from(&settings.comfy_path);
    let configured_model_root = PathBuf::from(&settings.model_path);
    let comfy_root = if locate_runtime_layout(&configured_comfy_root).is_some() {
        configured_comfy_root
    } else {
        discover_comfy_root(&disks).unwrap_or(configured_comfy_root)
    };
    let configured_model_count = count_ready_models(&configured_model_root, true);
    let model_root = discover_model_root(&disks, true)
        .filter(|path| count_ready_models(path, true) > configured_model_count)
        .unwrap_or(configured_model_root);
    let marker_path = comfy_root.join(".h3studio-install.json");
    let marker = fs::read(&marker_path)
        .await
        .ok()
        .and_then(|bytes| serde_json::from_slice::<InstallMarker>(&bytes).ok());
    let marker_runtime = marker
        .as_ref()
        .and_then(|value| locate_runtime_layout(Path::new(&value.runtime_path)));
    let runtime = marker_runtime.or_else(|| locate_runtime_layout(&comfy_root));
    let installed_models = MODELS
        .iter()
        .filter_map(|model| model_match(&model_root, model))
        .collect::<Vec<_>>();
    let required_base_count = MODELS.iter().filter(|model| !model.reference_only).count();
    let installed_base_count = MODELS
        .iter()
        .filter(|model| !model.reference_only && model_file_ready(&model_root, model))
        .count();
    let dependencies_ready = runtime
        .as_ref()
        .map(runtime_dependencies_ready)
        .unwrap_or(false);
    let configuration = detect_model_configuration(runtime.as_ref(), &model_root).await;
    let healthy = runtime.is_some()
        && dependencies_ready
        && configuration.ready
        && installed_base_count == required_base_count;
    let installed = runtime.is_some() || !installed_models.is_empty();
    let marker_exists = marker.is_some();
    InstallationStatus {
        installed,
        healthy,
        comfy_version: runtime
            .as_ref()
            .and_then(read_comfy_version)
            .or_else(|| marker.as_ref().map(|value| value.comfy_version.clone())),
        installed_models,
        message: if healthy {
            if marker_exists {
                "安装文件完整，可以启动本地引擎。".into()
            } else {
                "检测到已有 ComfyUI 与完整 H3 模型，可直接接管并写入配置。".into()
            }
        } else if runtime.is_none() && installed_base_count == 0 {
            "未在所选目录检测到 ComfyUI 或 MiniMax H3 模型。".into()
        } else if runtime.is_none() {
            format!(
                "检测到 {installed_base_count}/{required_base_count} 个基础模型，但未找到完整的 ComfyUI 环境。"
            )
        } else if !dependencies_ready {
            "已检测到 ComfyUI 与模型，但现有 Python 依赖需要修复。".into()
        } else if !configuration.ready {
            configuration.message
        } else {
            format!(
                "已检测到 ComfyUI，基础模型为 {installed_base_count}/{required_base_count}，安装器将补齐缺失文件。"
            )
        },
        runtime_path: runtime.map(|layout| layout.root.to_string_lossy().to_string()),
    }
}

async fn download_file(
    app: &AppHandle,
    state: &AppState,
    client: &reqwest::Client,
    spec: &DownloadSpec,
    completed_bytes: u64,
    total_bytes: u64,
) -> Result<bool, String> {
    if let Ok(metadata) = fs::metadata(&spec.destination).await
        && metadata.len() == spec.bytes
        && verify_sha256(
            app,
            state,
            &spec.destination,
            spec,
            completed_bytes,
            total_bytes,
        )
        .await?
    {
        return Ok(true);
    }

    if let Some(parent) = spec.destination.parent() {
        fs::create_dir_all(parent)
            .await
            .map_err(io_error("无法创建下载目录"))?;
    }
    let part_path = part_path(&spec.destination);
    let mut existing = fs::metadata(&part_path)
        .await
        .map(|meta| meta.len())
        .unwrap_or(0);
    if existing > spec.bytes {
        fs::remove_file(&part_path)
            .await
            .map_err(io_error("无法清理无效的断点文件"))?;
        existing = 0;
    }

    let mut request = client.get(&spec.url).header(ACCEPT_ENCODING, "identity");
    if existing > 0 {
        request = request.header(RANGE, format!("bytes={existing}-"));
    }
    let response = request
        .send()
        .await
        .map_err(|error| format!("下载 {} 失败：{error}", spec.name))?;
    if !response.status().is_success() {
        return Err(format!(
            "下载 {} 失败：服务器返回 {}。",
            spec.name,
            response.status()
        ));
    }

    let resumed = existing > 0 && response.status() == reqwest::StatusCode::PARTIAL_CONTENT;
    if existing > 0 && !resumed {
        fs::remove_file(&part_path)
            .await
            .map_err(io_error("无法重置不支持续传的文件"))?;
        existing = 0;
    }
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&part_path)
        .await
        .map_err(io_error("无法写入下载文件"))?;
    let mut downloaded = existing;
    let mut stream = response.bytes_stream();
    let mut last_emit = Instant::now() - Duration::from_secs(1);

    while let Some(chunk) = stream.next().await {
        check_cancel(state)?;
        let chunk = chunk.map_err(|error| {
            format!("下载 {} 时连接中断：{error}。下次会从断点继续。", spec.name)
        })?;
        file.write_all(&chunk)
            .await
            .map_err(io_error("写入下载文件失败"))?;
        downloaded = downloaded.saturating_add(chunk.len() as u64);
        if last_emit.elapsed() >= Duration::from_millis(250) {
            let aggregate = completed_bytes.saturating_add(downloaded.min(spec.bytes));
            emit_progress(
                app,
                InstallProgress {
                    stage: "download".into(),
                    stage_label: "下载组件".into(),
                    file_name: Some(spec.name.clone()),
                    file_downloaded_bytes: downloaded,
                    file_total_bytes: spec.bytes,
                    total_downloaded_bytes: aggregate,
                    total_bytes,
                    percent: percent(aggregate, total_bytes) * 0.90,
                    message: if resumed {
                        "正在从上次中断的位置继续下载…".into()
                    } else {
                        "正在从官方源下载…".into()
                    },
                    can_cancel: true,
                },
            );
            last_emit = Instant::now();
        }
    }
    file.flush().await.map_err(io_error("刷新下载文件失败"))?;
    drop(file);

    let final_size = fs::metadata(&part_path)
        .await
        .map_err(io_error("无法读取下载文件"))?
        .len();
    if final_size != spec.bytes {
        return Err(format!(
            "{} 下载不完整：预期 {} 字节，实际 {} 字节。下次会继续下载。",
            spec.name, spec.bytes, final_size
        ));
    }
    if !verify_sha256(app, state, &part_path, spec, completed_bytes, total_bytes).await? {
        let invalid = part_path.with_extension(format!("invalid-{}", Utc::now().timestamp()));
        fs::rename(&part_path, &invalid)
            .await
            .map_err(io_error("无法隔离校验失败的文件"))?;
        return Err(format!(
            "{} 的 SHA-256 校验失败，已隔离损坏文件。请重试下载。",
            spec.name
        ));
    }
    if spec.destination.exists() {
        fs::remove_file(&spec.destination)
            .await
            .map_err(io_error("无法替换旧文件"))?;
    }
    fs::rename(&part_path, &spec.destination)
        .await
        .map_err(io_error("无法完成下载文件重命名"))?;
    Ok(false)
}

async fn verify_sha256(
    app: &AppHandle,
    state: &AppState,
    path: &Path,
    spec: &DownloadSpec,
    completed_bytes: u64,
    total_bytes: u64,
) -> Result<bool, String> {
    emit_progress(
        app,
        InstallProgress {
            stage: "verify".into(),
            stage_label: "校验文件".into(),
            file_name: Some(spec.name.clone()),
            file_downloaded_bytes: spec.bytes,
            file_total_bytes: spec.bytes,
            total_downloaded_bytes: completed_bytes + spec.bytes,
            total_bytes,
            percent: percent(completed_bytes + spec.bytes, total_bytes) * 0.90,
            message: "正在验证 SHA-256，确保模型未损坏或被替换…".into(),
            can_cancel: true,
        },
    );
    let mut file = File::open(path)
        .await
        .map_err(io_error("无法打开待校验文件"))?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0_u8; 8 * 1024 * 1024];
    loop {
        check_cancel(state)?;
        let read = file
            .read(&mut buffer)
            .await
            .map_err(io_error("读取待校验文件失败"))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hex::encode(hasher.finalize()).eq_ignore_ascii_case(&spec.sha256))
}

async fn extract_comfy(archive: &Path, destination: &Path) -> Result<(), String> {
    if locate_runtime(destination).is_some() {
        return Ok(());
    }
    let mut command = tokio::process::Command::new("tar");
    command
        .args(["-xf"])
        .arg(archive)
        .args(["-C"])
        .arg(destination)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped());
    #[cfg(windows)]
    command.creation_flags(0x0800_0000);
    let output = command
        .output()
        .await
        .map_err(|error| format!("无法启动系统解压工具：{error}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("解压 ComfyUI 失败：{}", stderr.trim()));
    }
    Ok(())
}

fn same_path(left: &Path, right: &Path) -> bool {
    left.to_string_lossy()
        .trim_end_matches(['\\', '/'])
        .eq_ignore_ascii_case(right.to_string_lossy().trim_end_matches(['\\', '/']))
}

fn marker_id(path: &Path) -> Option<String> {
    std::fs::read_to_string(path.join(".comfyui-desktop-2"))
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn find_desktop_data_root(source_root: &Path) -> Option<PathBuf> {
    let expected_id = marker_id(source_root)?;
    let disks = Disks::new_with_refreshed_list();
    for disk in disks.iter() {
        let Ok(entries) = std::fs::read_dir(disk.mount_point()) else {
            continue;
        };
        for entry in entries.flatten() {
            let candidate = entry.path();
            if !candidate.is_dir()
                || !candidate.join(".venv/Scripts/python.exe").is_file()
                || marker_id(&candidate).as_deref() != Some(expected_id.as_str())
            {
                continue;
            }
            return Some(candidate);
        }
    }
    None
}

fn portable_layout(root: PathBuf) -> Option<RuntimeLayout> {
    let python = root.join("python_embeded/python.exe");
    let main_py = root.join("ComfyUI/main.py");
    if !python.is_file() || !main_py.is_file() {
        return None;
    }
    Some(RuntimeLayout {
        root: root.clone(),
        python,
        main_py,
        working_directory: root,
        kind: "portable",
    })
}

pub fn locate_runtime_layout(comfy_root: &Path) -> Option<RuntimeLayout> {
    if let Some(layout) = portable_layout(comfy_root.to_path_buf()) {
        return Some(layout);
    }
    if let Some(layout) = portable_layout(comfy_root.join("ComfyUI_windows_portable")) {
        return Some(layout);
    }

    let main_py = comfy_root.join("ComfyUI/main.py");
    if main_py.is_file() {
        let data_root = if comfy_root.join(".venv/Scripts/python.exe").is_file() {
            Some(comfy_root.to_path_buf())
        } else {
            find_desktop_data_root(comfy_root)
        }?;
        let python = data_root.join(".venv/Scripts/python.exe");
        return Some(RuntimeLayout {
            root: comfy_root.to_path_buf(),
            python,
            working_directory: main_py.parent()?.to_path_buf(),
            main_py,
            kind: "desktop",
        });
    }
    None
}

pub fn locate_runtime(comfy_root: &Path) -> Option<PathBuf> {
    locate_runtime_layout(comfy_root).map(|layout| layout.root)
}

fn read_comfy_version(layout: &RuntimeLayout) -> Option<String> {
    let contents =
        std::fs::read_to_string(layout.main_py.parent()?.join("comfyui_version.py")).ok()?;
    contents.lines().find_map(|line| {
        let line = line.trim();
        if !line.starts_with("__version__") {
            return None;
        }
        line.split_once('=')
            .map(|(_, value)| value.trim().trim_matches(['\'', '"']).to_string())
    })
}

fn runtime_dependencies_ready(layout: &RuntimeLayout) -> bool {
    let script = r#"from importlib.metadata import version
def parts(value):
    return tuple(int(p) for p in value.split('.')[:3])
assert parts(version('comfy-kitchen')) >= (0, 2, 26)
version('comfyui-frontend-package')
version('comfyui-workflow-templates')
version('comfy-angle')
print('ok')"#;
    command_version(&layout.python, &["-c", script]).as_deref() == Some("ok")
}

fn discover_comfy_root(disks: &Disks) -> Option<PathBuf> {
    let mut candidates = Vec::new();
    for disk in disks.iter() {
        let mount = disk.mount_point();
        candidates.extend([
            mount
                .join("Comfy-Desktop")
                .join("ComfyUI-Installs")
                .join("ComfyUI"),
            mount.join("ComfyUI"),
            mount.join("comfyui"),
            mount.join("MagineStudio").join("ComfyUI"),
            mount.join("H3Studio").join("ComfyUI"),
            mount.join("AI_Tools").join("ComfyUI"),
        ]);
    }
    candidates
        .into_iter()
        .find(|candidate| locate_runtime_layout(candidate).is_some())
}

fn count_ready_models(model_root: &Path, include_reference_model: bool) -> usize {
    MODELS
        .iter()
        .filter(|model| !model.reference_only || include_reference_model)
        .filter(|model| model_file_ready(model_root, model))
        .count()
}

fn discover_model_root(disks: &Disks, include_reference_model: bool) -> Option<PathBuf> {
    let mut best: Option<(usize, PathBuf)> = None;
    for disk in disks.iter() {
        let mount = disk.mount_point();
        for candidate in [
            mount.join("MiniMax-H3-Models"),
            mount.join("MagineStudio").join("Models"),
            mount.join("H3Studio").join("Models"),
            mount.join("AI_Models").join("MiniMax"),
            mount.join("AI_Tools").join("Models"),
        ] {
            let score = count_ready_models(&candidate, include_reference_model);
            if score > best.as_ref().map(|(score, _)| *score).unwrap_or(0) {
                best = Some((score, candidate));
            }
        }
    }
    best.map(|(_, path)| path)
}

async fn repair_runtime_dependencies(
    app: &AppHandle,
    layout: &RuntimeLayout,
    total_bytes: u64,
) -> Result<(), String> {
    let requirements = layout
        .main_py
        .parent()
        .ok_or_else(|| "无法确定 ComfyUI requirements.txt 位置。".to_string())?
        .join("requirements.txt");
    if !requirements.is_file() {
        return Err(format!(
            "检测到 ComfyUI，但缺少依赖清单：{}。",
            requirements.display()
        ));
    }
    emit_progress(
        app,
        InstallProgress {
            stage: "repair".into(),
            stage_label: "修复 ComfyUI 依赖".into(),
            file_name: Some("requirements.txt".into()),
            file_downloaded_bytes: 0,
            file_total_bytes: 0,
            total_downloaded_bytes: total_bytes,
            total_bytes,
            percent: 96.0,
            message: "正在使用现有独立 Python 补齐 ComfyUI 官方依赖，请勿关闭客户端。".into(),
            can_cancel: false,
        },
    );
    let mut command = tokio::process::Command::new(&layout.python);
    command
        .args(["-m", "pip", "install", "--disable-pip-version-check", "-r"])
        .arg(&requirements)
        .current_dir(&layout.working_directory)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped());
    #[cfg(windows)]
    command.creation_flags(0x0800_0000);
    let output = command
        .output()
        .await
        .map_err(|error| format!("无法启动 ComfyUI 依赖修复：{error}"))?;
    if !output.status.success() {
        let details = String::from_utf8_lossy(&output.stderr);
        let tail = details
            .lines()
            .rev()
            .take(6)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect::<Vec<_>>()
            .join(" ");
        return Err(format!("ComfyUI 依赖修复失败：{tail}"));
    }
    if !runtime_dependencies_ready(layout) {
        return Err("依赖安装命令已完成，但 ComfyUI 关键组件仍未通过检测。".into());
    }
    Ok(())
}

async fn configure_model_paths(layout: &RuntimeLayout, model_root: &Path) -> Result<(), String> {
    let config_path = layout
        .main_py
        .parent()
        .ok_or_else(|| "无法确定 ComfyUI 配置目录。".to_string())?
        .join("extra_model_paths.yaml");
    let start_marker = "# >>> MagineStudio managed model paths >>>";
    let end_marker = "# <<< MagineStudio managed model paths <<<";
    let existing = fs::read_to_string(&config_path).await.unwrap_or_default();
    let mut preserved = existing;
    for (managed_start, managed_end) in [
        (start_marker, end_marker),
        (
            "# >>> H3 Studio managed model paths >>>",
            "# <<< H3 Studio managed model paths <<<",
        ),
    ] {
        if let (Some(start), Some(end)) =
            (preserved.find(managed_start), preserved.find(managed_end))
            && end >= start
        {
            let end_index = end + managed_end.len();
            preserved = format!("{}{}", &preserved[..start], &preserved[end_index..]);
        }
    }
    let base = model_root
        .to_string_lossy()
        .replace('\\', "/")
        .replace('"', "\\\"");
    let managed = format!(
        "{start_marker}\nmaginestudio:\n  base_path: \"{base}\"\n  diffusion_models: |\n    diffusion_models\n    unet\n  unet: unet\n  text_encoders: text_encoders\n  vae: vae\n{end_marker}\n"
    );
    let content = format!("{}\n{}", preserved.trim_end(), managed);
    fs::write(&config_path, content)
        .await
        .map_err(io_error("无法写入模型路径配置"))
}

fn validate_request_paths(request: &InstallRequest) -> Result<(), String> {
    validate_target_path("ComfyUI", Path::new(&request.comfy_path))?;
    validate_target_path("模型", Path::new(&request.model_path))?;
    validate_target_path("输出", Path::new(&request.output_path))?;
    Ok(())
}

fn validate_target_path(label: &str, path: &Path) -> Result<(), String> {
    if !path.is_absolute() {
        return Err(format!("{label}目录必须是完整的绝对路径。"));
    }
    if path.parent().is_none() {
        return Err(format!("{label}目录不能直接使用磁盘根目录。"));
    }
    let normalized = path
        .to_string_lossy()
        .trim_end_matches(['\\', '/'])
        .to_ascii_lowercase();
    for blocked in [
        std::env::var("WINDIR").ok(),
        std::env::var("ProgramFiles").ok(),
        std::env::var("ProgramFiles(x86)").ok(),
        std::env::var("ProgramData").ok(),
    ]
    .into_iter()
    .flatten()
    {
        let blocked = blocked.trim_end_matches(['\\', '/']).to_ascii_lowercase();
        if normalized == blocked || normalized.starts_with(&(blocked + "\\")) {
            return Err(format!(
                "{label}目录不能位于 Windows 或 Program Files 系统目录中。"
            ));
        }
    }
    Ok(())
}

async fn preflight_space(request: &InstallRequest) -> Result<(), String> {
    let settings = AppSettings {
        comfy_path: request.comfy_path.clone(),
        model_path: request.model_path.clone(),
        output_path: request.output_path.clone(),
        enabled_models: None,
        license_accepted: request.license_accepted,
        region_confirmed: request.region_confirmed,
        accepted_license_version: None,
        accepted_at: None,
    };
    let scan = environment_report(&settings, request.include_reference_model).await;
    let failures = scan
        .path_spaces
        .iter()
        .filter(|space| !space.sufficient)
        .map(|space| format!("{}目录：{}", space.label, space.message))
        .collect::<Vec<_>>();
    if failures.is_empty() {
        Ok(())
    } else {
        Err(failures.join("\n"))
    }
}

fn part_path(destination: &Path) -> PathBuf {
    let file_name = destination
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("download");
    destination.with_file_name(format!("{file_name}.part"))
}

fn percent(downloaded: u64, total: u64) -> f64 {
    if total == 0 {
        0.0
    } else {
        downloaded as f64 / total as f64 * 100.0
    }
}

fn emit_progress(app: &AppHandle, progress: InstallProgress) {
    let _ = app.emit("install-progress", progress);
}

fn check_cancel(state: &AppState) -> Result<(), String> {
    if state.install_cancel.load(Ordering::SeqCst) {
        Err("安装已安全停止。已完成和未完成的下载都会保留，下次可继续。".into())
    } else {
        Ok(())
    }
}

fn io_error(context: &'static str) -> impl FnOnce(std::io::Error) -> String {
    move |error| format!("{context}：{error}")
}

struct InstallGuard<'a> {
    state: &'a AppState,
}

impl Drop for InstallGuard<'_> {
    fn drop(&mut self) {
        self.state.install_running.store(false, Ordering::SeqCst);
        self.state.install_cancel.store(false, Ordering::SeqCst);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEST_ALTERNATIVES: &[ModelAlternative] = &[ModelAlternative {
        relative_path: "unet/model-fp8.safetensors",
        bytes: 4,
    }];

    fn temp_root(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!("magine-studio-{label}-{}", std::process::id()))
    }

    fn request() -> InstallRequest {
        InstallRequest {
            comfy_path: r"C:\Users\Public\MagineStudio\ComfyUI".into(),
            model_path: r"C:\Users\Public\MagineStudio\Models".into(),
            output_path: r"C:\Users\Public\MagineStudio\Outputs".into(),
            include_reference_model: false,
            license_accepted: true,
            region_confirmed: true,
            license_version: LICENSE_VERSION.into(),
        }
    }

    #[test]
    fn base_plan_contains_four_model_files() {
        let plan = plan(&request()).expect("valid plan");
        assert_eq!(plan.model_files.len(), 4);
        assert_eq!(plan.download_bytes, 44_537_740_761);
    }

    #[test]
    fn reference_plan_adds_checkpoint() {
        let mut req = request();
        req.include_reference_model = true;
        let plan = plan(&req).expect("valid plan");
        assert_eq!(plan.model_files.len(), 5);
        assert_eq!(plan.download_bytes, 65_508_120_377);
    }

    #[test]
    fn disk_root_is_rejected() {
        let mut req = request();
        req.model_path = r"C:\".into();
        assert!(plan(&req).is_err());
    }

    #[test]
    fn accepted_model_variant_counts_as_ready() {
        let root = temp_root("model-variant");
        let relative = "unet/model-fp8.safetensors";
        let destination = relative_destination(&root, relative);
        std::fs::create_dir_all(destination.parent().expect("parent")).expect("create parent");
        std::fs::write(&destination, [1_u8, 2, 3, 4]).expect("write model");
        let spec = ModelSpec {
            id: "test",
            relative_path: "diffusion_models/preferred.safetensors",
            bytes: 8,
            sha256: "",
            reference_only: false,
            alternatives: TEST_ALTERNATIVES,
        };
        assert_eq!(model_match(&root, &spec).as_deref(), Some(relative));
        std::fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn portable_runtime_layout_is_detected() {
        let root = temp_root("portable-runtime");
        std::fs::create_dir_all(root.join("python_embeded")).expect("python directory");
        std::fs::create_dir_all(root.join("ComfyUI")).expect("comfy directory");
        std::fs::write(root.join("python_embeded/python.exe"), []).expect("python");
        std::fs::write(root.join("ComfyUI/main.py"), []).expect("main");
        let layout = locate_runtime_layout(&root).expect("runtime layout");
        assert_eq!(layout.kind, "portable");
        std::fs::remove_dir_all(root).expect("cleanup");
    }
}

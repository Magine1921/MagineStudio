use std::fs::OpenOptions;
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use std::time::Duration;
use tokio::time::sleep;

use crate::comfy_api;
use crate::installer;
use crate::models::{AppSettings, ComfyStatus};
use crate::state::AppState;

pub async fn start(state: &AppState, settings: &AppSettings) -> Result<ComfyStatus, String> {
    let requested_output = normalize_path(&settings.output_path);
    let current_url = state
        .comfy_url
        .lock()
        .map_err(|_| "无法读取本地引擎地址。".to_string())?
        .clone();
    let current_output = state
        .comfy_output_path
        .lock()
        .map_err(|_| "无法读取本地引擎输出目录。".to_string())?
        .clone();

    if current_output.as_deref() == Some(requested_output.as_str())
        && let Some(url) = current_url.as_deref()
        && comfy_api::is_ready_at(url).await
    {
        let pid = state
            .comfy_process
            .lock()
            .ok()
            .and_then(|guard| guard.as_ref().map(std::process::Child::id));
        return Ok(ComfyStatus {
            state: "ready".into(),
            message: format!(
                "MiniMax H3 本地引擎已就绪，输出目录：{}",
                settings.output_path
            ),
            pid,
            url: url.to_string(),
        });
    }

    {
        let mut guard = state
            .comfy_process
            .lock()
            .map_err(|_| "无法读取本地引擎状态。".to_string())?;
        if let Some(child) = guard.as_mut() {
            match child.try_wait() {
                Ok(None) => {
                    child
                        .kill()
                        .map_err(|error| format!("无法重启 ComfyUI 以应用输出目录：{error}"))?;
                    let _ = child.wait();
                    *guard = None;
                }
                Ok(Some(status)) => {
                    *state
                        .comfy_last_error
                        .lock()
                        .map_err(|_| "无法记录引擎错误。".to_string())? =
                        Some(format!("ComfyUI 上次以 {status} 退出。"));
                    *guard = None;
                }
                Err(error) => return Err(format!("无法检查 ComfyUI 进程：{error}")),
            }
        }
    }
    *state
        .comfy_url
        .lock()
        .map_err(|_| "无法更新本地引擎地址。".to_string())? = None;
    *state
        .comfy_output_path
        .lock()
        .map_err(|_| "无法更新本地引擎输出目录。".to_string())? = None;

    let installation = installer::status(settings).await;
    if !installation.installed || !installation.healthy {
        return Err(format!("本地生成环境尚未准备好：{}", installation.message));
    }
    let runtime_root = installation
        .runtime_path
        .map(PathBuf::from)
        .ok_or_else(|| "安装记录中没有 ComfyUI 运行路径。".to_string())?;
    let layout = installer::locate_runtime_layout(&runtime_root)
        .ok_or_else(|| "无法配对 ComfyUI 主程序与独立 Python 环境。".to_string())?;
    let output = PathBuf::from(&settings.output_path);
    std::fs::create_dir_all(&output).map_err(|error| format!("无法创建输出目录：{error}"))?;

    let logs = PathBuf::from(&settings.comfy_path).join("logs");
    std::fs::create_dir_all(&logs).map_err(|error| format!("无法创建日志目录：{error}"))?;
    let log_path = logs.join("comfyui.log");
    let stdout = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
        .map_err(|error| format!("无法打开 ComfyUI 日志：{error}"))?;
    let stderr = stdout
        .try_clone()
        .map_err(|error| format!("无法准备 ComfyUI 日志：{error}"))?;

    let port = available_managed_port()?;
    let managed_url = format!("http://127.0.0.1:{port}");
    let managed_user_directory = PathBuf::from(&settings.comfy_path).join(".h3studio-user");
    std::fs::create_dir_all(&managed_user_directory)
        .map_err(|error| format!("无法创建 MagineStudio 独立用户目录：{error}"))?;
    let database_path = managed_user_directory.join("comfyui.db");
    let database_url = format!(
        "sqlite:///{}",
        database_path.to_string_lossy().replace('\\', "/")
    );

    let mut command = Command::new(&layout.python);
    command.current_dir(&layout.working_directory);
    if layout.kind == "portable" {
        command.arg("-s");
    }
    command.arg(&layout.main_py);
    if layout.kind == "portable" {
        command.arg("--windows-standalone-build");
    }
    command
        .arg("--user-directory")
        .arg(&managed_user_directory)
        .arg("--database-url")
        .arg(database_url);
    if let Some(config_path) = layout
        .main_py
        .parent()
        .map(|root| root.join("extra_model_paths.yaml"))
        && config_path.is_file()
    {
        command.arg("--extra-model-paths-config").arg(config_path);
    }
    command
        .arg("--listen")
        .arg("127.0.0.1")
        .arg("--port")
        .arg(port.to_string())
        .arg("--enable-cors-header")
        .arg("--disable-auto-launch")
        .arg("--output-directory")
        .arg(&output)
        .stdin(Stdio::null())
        .stdout(Stdio::from(stdout))
        .stderr(Stdio::from(stderr));
    hide_console(&mut command);
    let child = command
        .spawn()
        .map_err(|error| format!("无法启动 ComfyUI：{error}"))?;
    let pid = child.id();
    *state
        .comfy_process
        .lock()
        .map_err(|_| "无法保存 ComfyUI 进程状态。".to_string())? = Some(child);
    *state
        .comfy_url
        .lock()
        .map_err(|_| "无法保存本地引擎地址。".to_string())? = Some(managed_url.clone());
    *state
        .comfy_output_path
        .lock()
        .map_err(|_| "无法保存本地引擎输出目录。".to_string())? = Some(requested_output);
    *state
        .comfy_last_error
        .lock()
        .map_err(|_| "无法清理引擎错误状态。".to_string())? = None;

    for _ in 0..120 {
        if comfy_api::is_ready_at(&managed_url).await {
            return Ok(ComfyStatus {
                state: "ready".into(),
                message: format!(
                    "MiniMax H3 本地引擎已就绪，输出目录：{}",
                    settings.output_path
                ),
                pid: Some(pid),
                url: managed_url,
            });
        }
        let exited = {
            let mut guard = state
                .comfy_process
                .lock()
                .map_err(|_| "无法检查 ComfyUI 进程。".to_string())?;
            match guard.as_mut() {
                Some(child) => match child.try_wait() {
                    Ok(Some(status)) => Some(status.to_string()),
                    Ok(None) => None,
                    Err(error) => Some(error.to_string()),
                },
                None => Some("进程状态丢失".into()),
            }
        };
        if let Some(reason) = exited {
            let message = format!(
                "ComfyUI 启动失败（{reason}）。请查看 {}。",
                log_path.display()
            );
            *state
                .comfy_last_error
                .lock()
                .map_err(|_| "无法记录启动错误。".to_string())? = Some(message.clone());
            *state
                .comfy_process
                .lock()
                .map_err(|_| "无法清理进程状态。".to_string())? = None;
            return Err(message);
        }
        sleep(Duration::from_secs(1)).await;
    }
    Err(format!(
        "ComfyUI 在 120 秒内没有完成启动。请查看 {}，确认显卡驱动和模型环境。",
        log_path.display()
    ))
}

pub fn stop(state: &AppState) -> Result<ComfyStatus, String> {
    let mut guard = state
        .comfy_process
        .lock()
        .map_err(|_| "无法读取本地引擎状态。".to_string())?;
    if let Some(mut child) = guard.take() {
        child
            .kill()
            .map_err(|error| format!("无法停止 ComfyUI：{error}"))?;
        let _ = child.wait();
    }
    *state
        .comfy_url
        .lock()
        .map_err(|_| "无法清理本地引擎地址。".to_string())? = None;
    *state
        .comfy_output_path
        .lock()
        .map_err(|_| "无法清理本地引擎输出目录。".to_string())? = None;
    Ok(ComfyStatus {
        state: "stopped".into(),
        message: "本地引擎已停止。".into(),
        pid: None,
        url: comfy_api::MANAGED_URL.into(),
    })
}

pub async fn status(state: &AppState) -> ComfyStatus {
    if let Some(url) = active_url(state).await {
        let pid = state
            .comfy_process
            .lock()
            .ok()
            .and_then(|guard| guard.as_ref().map(std::process::Child::id));
        return ComfyStatus {
            state: "ready".into(),
            message: "本地引擎已就绪。".into(),
            pid,
            url,
        };
    }
    let process_state = state.comfy_process.lock().ok().and_then(|mut guard| {
        guard.as_mut().map(|child| match child.try_wait() {
            Ok(None) => (
                "starting".to_string(),
                "ComfyUI 正在启动。".to_string(),
                Some(child.id()),
            ),
            Ok(Some(code)) => ("error".to_string(), format!("ComfyUI 已退出：{code}"), None),
            Err(error) => (
                "error".to_string(),
                format!("无法检查 ComfyUI：{error}"),
                None,
            ),
        })
    });
    if let Some((state_name, message, pid)) = process_state {
        return ComfyStatus {
            state: state_name,
            message,
            pid,
            url: comfy_api::MANAGED_URL.into(),
        };
    }
    let last_error = state
        .comfy_last_error
        .lock()
        .ok()
        .and_then(|guard| guard.clone());
    ComfyStatus {
        state: if last_error.is_some() {
            "error"
        } else {
            "stopped"
        }
        .into(),
        message: last_error.unwrap_or_else(|| "尚未启动。".into()),
        pid: None,
        url: comfy_api::MANAGED_URL.into(),
    }
}

pub async fn active_url(state: &AppState) -> Option<String> {
    let managed_url = state.comfy_url.lock().ok().and_then(|guard| guard.clone());
    if let Some(url) = managed_url
        && comfy_api::is_ready_at(&url).await
    {
        return Some(url);
    }
    comfy_api::detect_base_url().await
}

pub fn kill_on_exit(state: &AppState) {
    if let Ok(mut guard) = state.comfy_process.lock()
        && let Some(mut child) = guard.take()
    {
        let _ = child.kill();
        let _ = child.wait();
    }
}

fn available_managed_port() -> Result<u16, String> {
    (8188..=8198)
        .find(|port| TcpListener::bind(("127.0.0.1", *port)).is_ok())
        .ok_or_else(|| "无法启动本地引擎：8188–8198 端口均被占用。".to_string())
}

fn normalize_path(path: &str) -> String {
    path.trim_end_matches(['\\', '/'])
        .replace('/', "\\")
        .to_ascii_lowercase()
}

#[cfg(windows)]
fn hide_console(command: &mut Command) {
    use std::os::windows::process::CommandExt;
    command.creation_flags(0x0800_0000);
}

#[cfg(not(windows))]
fn hide_console(_command: &mut Command) {}

pub fn open_folder(path: &Path) -> Result<(), String> {
    if !path.exists() {
        std::fs::create_dir_all(path).map_err(|error| format!("无法创建输出目录：{error}"))?;
    }
    let mut command = Command::new("explorer.exe");
    command.arg(path);
    command
        .spawn()
        .map_err(|error| format!("无法打开文件夹：{error}"))?;
    Ok(())
}

pub fn open_url(url: &str) -> Result<(), String> {
    const ALLOWED: &[&str] = &[
        "https://huggingface.co/MiniMaxAI/MiniMax-H3/blob/main/LICENSE",
        "https://docs.comfy.org/tutorials/video/minimax/minimax-h3",
        "https://minimaxi.com/blog/minimax-h3",
        "https://www.python.org/doc/",
        "https://www.nvidia.com/Download/index.aspx?lang=cn",
    ];
    if !ALLOWED.contains(&url) && !is_allowed_local_comfy_url(url) {
        return Err("出于安全原因，只允许打开应用内列出的官方页面。".into());
    }
    Command::new("explorer.exe")
        .arg(url)
        .spawn()
        .map_err(|error| format!("无法打开浏览器：{error}"))?;
    Ok(())
}

fn is_allowed_local_comfy_url(url: &str) -> bool {
    let Ok(parsed) = reqwest::Url::parse(url) else {
        return false;
    };
    let local_host = matches!(parsed.host_str(), Some("127.0.0.1" | "localhost"));
    let allowed_port = matches!(parsed.port_or_known_default(), Some(8000 | 8188..=8198));
    parsed.scheme() == "http" && local_host && allowed_port && parsed.username().is_empty()
}

#[cfg(test)]
mod tests {
    use super::is_allowed_local_comfy_url;

    #[test]
    fn local_comfy_url_allowlist_rejects_remote_and_credentialed_urls() {
        assert!(is_allowed_local_comfy_url("http://127.0.0.1:8188"));
        assert!(is_allowed_local_comfy_url("http://localhost:8000/"));
        assert!(!is_allowed_local_comfy_url("https://127.0.0.1:8188"));
        assert!(!is_allowed_local_comfy_url("http://example.com:8188"));
        assert!(!is_allowed_local_comfy_url("http://user@127.0.0.1:8188"));
        assert!(!is_allowed_local_comfy_url("http://127.0.0.1:9000"));
    }
}

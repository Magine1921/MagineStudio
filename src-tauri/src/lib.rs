mod comfy;
mod comfy_api;
mod hardware;
mod installer;
mod models;
mod runtime;
mod state;

use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;

use tauri::{AppHandle, Manager, State};

use models::{
    AppSettings, ComfyStatus, EnvironmentReport, GenerationJob, GenerationRequest, HardwareReport,
    InstallPlan, InstallRequest, InstallationStatus,
};
use state::AppState;

#[tauri::command]
fn scan_hardware() -> HardwareReport {
    hardware::scan()
}

#[tauri::command]
async fn scan_environment(
    settings: AppSettings,
    include_reference_model: bool,
) -> EnvironmentReport {
    installer::environment_report(&settings, include_reference_model).await
}

#[tauri::command]
async fn load_settings(app: AppHandle) -> Result<AppSettings, String> {
    let path = settings_path(&app)?;
    match tokio::fs::read(&path).await {
        Ok(bytes) => parse_settings(&bytes),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            migrate_legacy_settings(&app, &path).await
        }
        Err(error) => Err(format!("无法读取设置：{error}")),
    }
}

fn parse_settings(bytes: &[u8]) -> Result<AppSettings, String> {
    serde_json::from_slice(bytes).map_err(|error| format!("设置文件格式无效：{error}"))
}

async fn migrate_legacy_settings(app: &AppHandle, path: &Path) -> Result<AppSettings, String> {
    let legacy_path = app
        .path()
        .config_dir()
        .map(|directory| directory.join("com.h3studio.desktop").join("settings.json"))
        .map_err(|error| format!("无法确定旧版设置目录：{error}"))?;
    let bytes = match tokio::fs::read(&legacy_path).await {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(AppSettings::default());
        }
        Err(error) => return Err(format!("无法读取旧版设置：{error}")),
    };
    let settings = parse_settings(&bytes)?;
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|error| format!("无法创建新版设置目录：{error}"))?;
    }
    tokio::fs::write(path, bytes)
        .await
        .map_err(|error| format!("无法迁移旧版设置：{error}"))?;
    Ok(settings)
}

#[tauri::command]
fn get_default_settings() -> AppSettings {
    AppSettings::default()
}

#[tauri::command]
async fn save_settings(app: AppHandle, settings: AppSettings) -> Result<(), String> {
    validate_settings(&settings)?;
    let path = settings_path(&app)?;
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|error| format!("无法创建设置目录：{error}"))?;
    }
    let bytes =
        serde_json::to_vec_pretty(&settings).map_err(|error| format!("无法生成设置：{error}"))?;
    tokio::fs::write(path, bytes)
        .await
        .map_err(|error| format!("无法保存设置：{error}"))
}

#[tauri::command]
fn create_install_plan(request: InstallRequest) -> Result<InstallPlan, String> {
    installer::plan(&request)
}

#[tauri::command]
async fn start_install(
    app: AppHandle,
    state: State<'_, AppState>,
    request: InstallRequest,
) -> Result<InstallationStatus, String> {
    installer::install(&app, state.inner(), request).await
}

#[tauri::command]
fn cancel_install(state: State<'_, AppState>) {
    state.install_cancel.store(true, Ordering::SeqCst);
}

#[tauri::command]
async fn get_installation_status(settings: AppSettings) -> InstallationStatus {
    installer::status(&settings).await
}

#[tauri::command]
async fn start_comfyui(
    state: State<'_, AppState>,
    settings: AppSettings,
) -> Result<ComfyStatus, String> {
    validate_settings(&settings)?;
    runtime::start(state.inner(), &settings).await
}

#[tauri::command]
fn stop_comfyui(state: State<'_, AppState>) -> Result<ComfyStatus, String> {
    runtime::stop(state.inner())
}

#[tauri::command]
async fn get_comfy_status(state: State<'_, AppState>) -> Result<ComfyStatus, String> {
    Ok(runtime::status(state.inner()).await)
}

#[tauri::command]
async fn queue_generation(
    state: State<'_, AppState>,
    request: GenerationRequest,
    settings: AppSettings,
) -> Result<GenerationJob, String> {
    comfy::queue(request, &settings, state.inner()).await
}

#[tauri::command]
async fn poll_generation(
    app: AppHandle,
    state: State<'_, AppState>,
    prompt_id: String,
    client_id: String,
    settings: AppSettings,
) -> Result<GenerationJob, String> {
    let job = comfy::poll(&prompt_id, &client_id, &settings, state.inner()).await?;
    if let Some(path) = job.local_video_path.as_deref() {
        app.asset_protocol_scope()
            .allow_file(path)
            .map_err(|error| format!("无法授权本地视频预览：{error}"))?;
    }
    Ok(job)
}

#[tauri::command]
async fn cancel_generation(state: State<'_, AppState>) -> Result<(), String> {
    comfy::cancel(state.inner()).await
}

#[tauri::command]
fn open_external(url: String) -> Result<(), String> {
    runtime::open_url(&url)
}

#[tauri::command]
fn open_output_folder(path: String) -> Result<(), String> {
    let path = PathBuf::from(path);
    if !path.is_absolute() || path.parent().is_none() {
        return Err("输出目录必须是完整路径，且不能是磁盘根目录。".into());
    }
    runtime::open_folder(&path)
}

#[tauri::command]
async fn write_text_file(path: String, contents: String) -> Result<(), String> {
    let path = PathBuf::from(path);
    if !path.is_absolute() || path.parent().is_none() {
        return Err("导出位置必须是完整文件路径。".into());
    }
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|error| format!("无法创建导出目录：{error}"))?;
    }
    tokio::fs::write(&path, contents.as_bytes())
        .await
        .map_err(|error| format!("无法写入导出文件：{error}"))
}

#[tauri::command]
async fn clear_cache(path: String) -> Result<(), String> {
    let path = PathBuf::from(path);
    if !is_managed_cache_path(&path) {
        return Err("仅允许清理 MagineStudio 管理的缓存目录。".into());
    }
    match tokio::fs::remove_dir_all(&path).await {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(format!("无法清理缓存：{error}")),
    }
    tokio::fs::create_dir_all(&path)
        .await
        .map_err(|error| format!("无法重新创建缓存目录：{error}"))
}

fn is_managed_cache_path(path: &Path) -> bool {
    let leaf = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    path.is_absolute() && path.parent().is_some() && leaf == ".h3studio-cache"
}

fn settings_path(app: &AppHandle) -> Result<PathBuf, String> {
    app.path()
        .app_config_dir()
        .map(|directory| directory.join("settings.json"))
        .map_err(|error| format!("无法确定设置目录：{error}"))
}

fn validate_settings(settings: &AppSettings) -> Result<(), String> {
    for (label, value) in [
        ("ComfyUI", &settings.comfy_path),
        ("模型", &settings.model_path),
        ("输出", &settings.output_path),
    ] {
        let path = Path::new(value);
        if !path.is_absolute() || path.parent().is_none() {
            return Err(format!("{label}目录必须是完整路径，且不能是磁盘根目录。"));
        }
    }
    Ok(())
}

pub fn run() {
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(AppState::default())
        .invoke_handler(tauri::generate_handler![
            scan_hardware,
            scan_environment,
            load_settings,
            get_default_settings,
            save_settings,
            create_install_plan,
            start_install,
            cancel_install,
            get_installation_status,
            start_comfyui,
            stop_comfyui,
            get_comfy_status,
            queue_generation,
            poll_generation,
            cancel_generation,
            open_external,
            open_output_folder,
            write_text_file,
            clear_cache,
        ])
        .build(tauri::generate_context!())
        .expect("failed to build MagineStudio");

    app.run(|handle, event| {
        if matches!(event, tauri::RunEvent::Exit) {
            let state = handle.state::<AppState>();
            runtime::kill_on_exit(state.inner());
        }
    });
}

#[cfg(test)]
mod tests {
    use super::is_managed_cache_path;
    use std::path::Path;

    #[test]
    fn installer_cache_is_allowed_but_regular_directories_are_rejected() {
        assert!(is_managed_cache_path(Path::new(
            r"C:\Users\Test\MagineStudio\ComfyUI\.h3studio-cache"
        )));
        assert!(!is_managed_cache_path(Path::new(
            r"C:\Users\Test\MagineStudio\ComfyUI\models"
        )));
        assert!(!is_managed_cache_path(Path::new(r"C:\Users\Test\cache")));
    }
}

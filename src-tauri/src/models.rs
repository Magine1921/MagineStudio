use serde::{Deserialize, Serialize};

pub const LICENSE_VERSION: &str = "2026-08-02";
pub const RULES_VERSION: &str = "2026.08.1";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GpuInfo {
    pub name: String,
    pub vendor: String,
    pub vram_mb: Option<u64>,
    pub driver_version: Option<String>,
    pub compute_capability: Option<String>,
    pub cuda_available: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiskInfo {
    pub name: String,
    pub mount_point: String,
    pub total_bytes: u64,
    pub available_bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EnvironmentComponentStatus {
    pub id: String,
    pub name: String,
    pub state: String,
    pub version: Option<String>,
    pub path: Option<String>,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PathSpaceInfo {
    pub id: String,
    pub label: String,
    pub path: String,
    pub mount_point: Option<String>,
    pub total_bytes: Option<u64>,
    pub available_bytes: Option<u64>,
    pub required_bytes: u64,
    pub sufficient: bool,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EnvironmentReport {
    pub scanned_at: String,
    pub components: Vec<EnvironmentComponentStatus>,
    pub path_spaces: Vec<PathSpaceInfo>,
    pub suggested_comfy_path: Option<String>,
    pub suggested_model_path: Option<String>,
    pub alignment_message: Option<String>,
    pub ready_count: usize,
    pub total_count: usize,
    pub summary: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HardwareCheck {
    pub id: String,
    pub label: String,
    pub value: String,
    pub status: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GenerationPreset {
    pub width: u32,
    pub height: u32,
    pub duration_seconds: u32,
    pub steps: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CapabilityTier {
    Comfortable,
    Usable,
    Experimental,
    HighRisk,
    Unsupported,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HardwareReport {
    pub scanned_at: String,
    pub os_name: String,
    pub os_version: String,
    pub architecture: String,
    pub cpu_name: String,
    pub physical_cores: Option<usize>,
    pub logical_cores: usize,
    pub total_memory_bytes: u64,
    pub available_memory_bytes: u64,
    pub gpus: Vec<GpuInfo>,
    pub disks: Vec<DiskInfo>,
    pub tier: CapabilityTier,
    pub tier_label: String,
    pub summary: String,
    pub checks: Vec<HardwareCheck>,
    pub recommendations: Vec<String>,
    pub preset: GenerationPreset,
    pub rules_version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppSettings {
    pub comfy_path: String,
    pub model_path: String,
    pub output_path: String,
    #[serde(default)]
    pub enabled_models: Option<Vec<String>>,
    pub license_accepted: bool,
    pub region_confirmed: bool,
    pub accepted_license_version: Option<String>,
    pub accepted_at: Option<String>,
}

impl Default for AppSettings {
    fn default() -> Self {
        let home = std::env::var_os("USERPROFILE")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| std::path::PathBuf::from(r"C:\Users\Public"));
        let root = home.join("MagineStudio");
        Self {
            comfy_path: root.join("ComfyUI").to_string_lossy().to_string(),
            model_path: root.join("Models").to_string_lossy().to_string(),
            output_path: root.join("Outputs").to_string_lossy().to_string(),
            enabled_models: None,
            license_accepted: false,
            region_confirmed: false,
            accepted_license_version: None,
            accepted_at: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallRequest {
    pub comfy_path: String,
    pub model_path: String,
    pub output_path: String,
    pub include_reference_model: bool,
    pub license_accepted: bool,
    pub region_confirmed: bool,
    pub license_version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallComponent {
    pub id: String,
    pub name: String,
    pub description: String,
    pub bytes: u64,
    pub required: bool,
    pub selected: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallPlan {
    pub components: Vec<InstallComponent>,
    pub download_bytes: u64,
    pub required_free_bytes: u64,
    pub warnings: Vec<String>,
    pub model_files: Vec<String>,
    pub comfy_version: String,
    pub license_version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallProgress {
    pub stage: String,
    pub stage_label: String,
    pub file_name: Option<String>,
    pub file_downloaded_bytes: u64,
    pub file_total_bytes: u64,
    pub total_downloaded_bytes: u64,
    pub total_bytes: u64,
    pub percent: f64,
    pub message: String,
    pub can_cancel: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallationStatus {
    pub installed: bool,
    pub healthy: bool,
    pub comfy_version: Option<String>,
    pub installed_models: Vec<String>,
    pub message: String,
    pub runtime_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallMarker {
    pub schema_version: u32,
    pub installed_at: String,
    pub comfy_version: String,
    pub license_version: String,
    pub runtime_path: String,
    pub model_path: String,
    pub model_files: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ComfyStatus {
    pub state: String,
    pub message: String,
    pub pid: Option<u32>,
    pub url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GenerationRequest {
    pub prompt: String,
    #[serde(default)]
    pub selected_model: Option<String>,
    pub width: u32,
    pub height: u32,
    pub duration_seconds: f64,
    #[serde(default = "default_generation_fps")]
    pub fps: u32,
    pub steps: u32,
    pub seed: i64,
    pub first_frame_path: Option<String>,
    #[serde(default)]
    pub last_frame_path: Option<String>,
    #[serde(default)]
    pub reference_paths: Vec<String>,
    pub output_prefix: String,
}

fn default_generation_fps() -> u32 {
    24
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GenerationJob {
    pub prompt_id: String,
    pub client_id: String,
    pub state: String,
    pub message: String,
    pub video_url: Option<String>,
    pub local_video_path: Option<String>,
    pub filename: Option<String>,
}

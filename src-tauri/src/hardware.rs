use std::process::Command;

use chrono::Utc;
use serde_json::Value;
use sysinfo::{Disks, System};

use crate::models::{
    CapabilityTier, DiskInfo, GenerationPreset, GpuInfo, HardwareCheck, HardwareReport,
    RULES_VERSION,
};

const GIB: u64 = 1024 * 1024 * 1024;

pub fn scan() -> HardwareReport {
    let mut system = System::new_all();
    system.refresh_all();

    let total_memory = system.total_memory();
    let available_memory = system.available_memory();
    let cpu_name = system
        .cpus()
        .first()
        .map(|cpu| cpu.brand().trim().to_string())
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| "未知处理器".to_string());
    let gpus = scan_nvidia_gpus().unwrap_or_else(scan_windows_gpus);
    let disks = scan_disks();
    let architecture = std::env::consts::ARCH.to_string();
    let (tier, tier_label, summary, preset) = evaluate_tier(&gpus, total_memory, &architecture);
    let checks = build_checks(&gpus, total_memory, &architecture);
    let recommendations = build_recommendations(&tier, total_memory);

    HardwareReport {
        scanned_at: Utc::now().to_rfc3339(),
        os_name: System::name().unwrap_or_else(|| "Windows".to_string()),
        os_version: System::os_version().unwrap_or_else(|| "未知版本".to_string()),
        architecture,
        cpu_name,
        physical_cores: System::physical_core_count(),
        logical_cores: system.cpus().len(),
        total_memory_bytes: total_memory,
        available_memory_bytes: available_memory,
        gpus,
        disks,
        tier,
        tier_label,
        summary,
        checks,
        recommendations,
        preset,
        rules_version: RULES_VERSION.to_string(),
    }
}

fn scan_disks() -> Vec<DiskInfo> {
    let disks = Disks::new_with_refreshed_list();
    disks
        .iter()
        .map(|disk| DiskInfo {
            name: disk.name().to_string_lossy().to_string(),
            mount_point: disk.mount_point().to_string_lossy().to_string(),
            total_bytes: disk.total_space(),
            available_bytes: disk.available_space(),
        })
        .collect()
}

fn scan_nvidia_gpus() -> Option<Vec<GpuInfo>> {
    let with_compute = Command::new("nvidia-smi")
        .args([
            "--query-gpu=name,memory.total,driver_version,compute_cap",
            "--format=csv,noheader,nounits",
        ])
        .output();

    let (output, has_compute) = match with_compute {
        Ok(result) if result.status.success() => (result, true),
        _ => {
            let result = Command::new("nvidia-smi")
                .args([
                    "--query-gpu=name,memory.total,driver_version",
                    "--format=csv,noheader,nounits",
                ])
                .output()
                .ok()?;
            if !result.status.success() {
                return None;
            }
            (result, false)
        }
    };

    let stdout = String::from_utf8_lossy(&output.stdout);
    let gpus: Vec<GpuInfo> = stdout
        .lines()
        .filter_map(|line| {
            let fields: Vec<&str> = line.split(',').map(str::trim).collect();
            if fields.len() < 3 {
                return None;
            }
            Some(GpuInfo {
                name: fields[0].to_string(),
                vendor: "NVIDIA".to_string(),
                vram_mb: fields[1].parse::<u64>().ok(),
                driver_version: Some(fields[2].to_string()),
                compute_capability: has_compute
                    .then(|| fields.get(3).unwrap_or(&"").to_string())
                    .filter(|v| !v.is_empty()),
                cuda_available: true,
            })
        })
        .collect();

    (!gpus.is_empty()).then_some(gpus)
}

fn scan_windows_gpus() -> Vec<GpuInfo> {
    let script = "Get-CimInstance Win32_VideoController | Select-Object Name,AdapterRAM,DriverVersion | ConvertTo-Json -Compress";
    let output = match Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", script])
        .output()
    {
        Ok(output) if output.status.success() => output,
        _ => return Vec::new(),
    };

    let value: Value = match serde_json::from_slice(&output.stdout) {
        Ok(value) => value,
        Err(_) => return Vec::new(),
    };
    let items: Vec<&Value> = match &value {
        Value::Array(values) => values.iter().collect(),
        Value::Object(_) => vec![&value],
        _ => Vec::new(),
    };

    items
        .into_iter()
        .filter_map(|item| {
            let name = item.get("Name")?.as_str()?.to_string();
            let lower = name.to_ascii_lowercase();
            let vendor = if lower.contains("nvidia") {
                "NVIDIA"
            } else if lower.contains("amd") || lower.contains("radeon") {
                "AMD"
            } else if lower.contains("intel") {
                "Intel"
            } else {
                "未知"
            };
            Some(GpuInfo {
                name,
                vendor: vendor.to_string(),
                vram_mb: item
                    .get("AdapterRAM")
                    .and_then(Value::as_u64)
                    .map(|bytes| bytes / 1024 / 1024),
                driver_version: item
                    .get("DriverVersion")
                    .and_then(Value::as_str)
                    .map(str::to_string),
                compute_capability: None,
                cuda_available: false,
            })
        })
        .collect()
}

pub fn evaluate_tier(
    gpus: &[GpuInfo],
    total_memory: u64,
    architecture: &str,
) -> (CapabilityTier, String, String, GenerationPreset) {
    let best_nvidia_vram = gpus
        .iter()
        .filter(|gpu| gpu.cuda_available)
        .filter_map(|gpu| gpu.vram_mb)
        .max()
        .unwrap_or(0);
    let memory_gib = total_memory / GIB;

    let tier = if architecture != "x86_64" || memory_gib < 16 || best_nvidia_vram < 6 * 1024 {
        CapabilityTier::Unsupported
    } else if best_nvidia_vram >= 24 * 1024 && memory_gib >= 64 {
        CapabilityTier::Comfortable
    } else if best_nvidia_vram >= 16 * 1024 && memory_gib >= 32 {
        CapabilityTier::Usable
    } else if best_nvidia_vram >= 8 * 1024 && memory_gib >= 32 {
        CapabilityTier::Experimental
    } else {
        CapabilityTier::HighRisk
    };

    match tier {
        CapabilityTier::Comfortable => (
            tier,
            "舒适运行".to_string(),
            "配置适合从 H3 原生 768p、5 秒开始，并逐步增加时长。".to_string(),
            GenerationPreset {
                width: 1344,
                height: 768,
                duration_seconds: 5,
                steps: 20,
            },
        ),
        CapabilityTier::Usable => (
            tier,
            "可以运行".to_string(),
            "可以使用模型卸载运行 H3，建议先从均衡分辨率开始。".to_string(),
            GenerationPreset {
                width: 960,
                height: 544,
                duration_seconds: 5,
                steps: 20,
            },
        ),
        CapabilityTier::Experimental => (
            tier,
            "实验性运行".to_string(),
            "预计可以低分辨率运行，但速度较慢并依赖系统内存与页面文件。".to_string(),
            GenerationPreset {
                width: 864,
                height: 480,
                duration_seconds: 4,
                steps: 16,
            },
        ),
        CapabilityTier::HighRisk => (
            tier,
            "高风险运行".to_string(),
            "只建议用于低分辨率测试，可能非常慢或因内存不足失败。".to_string(),
            GenerationPreset {
                width: 608,
                height: 352,
                duration_seconds: 4,
                steps: 12,
            },
        ),
        CapabilityTier::Unsupported => (
            tier,
            "不支持一键本地运行".to_string(),
            "未检测到满足首版要求的 NVIDIA CUDA 显卡、内存或 x64 Windows 环境。".to_string(),
            GenerationPreset {
                width: 608,
                height: 352,
                duration_seconds: 4,
                steps: 12,
            },
        ),
    }
}

fn build_checks(gpus: &[GpuInfo], total_memory: u64, architecture: &str) -> Vec<HardwareCheck> {
    let gpu = gpus
        .iter()
        .filter(|gpu| gpu.cuda_available)
        .max_by_key(|gpu| gpu.vram_mb.unwrap_or(0));
    let gpu_check = match gpu {
        Some(gpu) => {
            let vram = gpu.vram_mb.unwrap_or(0);
            HardwareCheck {
                id: "gpu".to_string(),
                label: "NVIDIA CUDA 显卡".to_string(),
                value: format!("{} · {:.0} GB", gpu.name, vram as f64 / 1024.0),
                status: if vram >= 16 * 1024 { "pass" } else { "warning" }.to_string(),
                message: if vram >= 16 * 1024 {
                    "满足首版可用档建议"
                } else {
                    "需要低显存卸载，速度和稳定性受限"
                }
                .to_string(),
            }
        }
        None => HardwareCheck {
            id: "gpu".to_string(),
            label: "NVIDIA CUDA 显卡".to_string(),
            value: "未检测到".to_string(),
            status: "fail".to_string(),
            message: "首版一键运行仅支持 NVIDIA CUDA".to_string(),
        },
    };

    let memory_gib = total_memory as f64 / GIB as f64;
    let memory_check = HardwareCheck {
        id: "memory".to_string(),
        label: "物理内存".to_string(),
        value: format!("{memory_gib:.0} GB"),
        status: if memory_gib >= 32.0 {
            "pass"
        } else if memory_gib >= 16.0 {
            "warning"
        } else {
            "fail"
        }
        .to_string(),
        message: if memory_gib >= 32.0 {
            "满足模型卸载的基础建议"
        } else {
            "建议至少 32 GB，并配置足够大的页面文件"
        }
        .to_string(),
    };

    let system_ok = architecture == "x86_64" && cfg!(target_os = "windows");
    let system_check = HardwareCheck {
        id: "system".to_string(),
        label: "操作系统".to_string(),
        value: format!("Windows · {architecture}"),
        status: if system_ok { "pass" } else { "fail" }.to_string(),
        message: if system_ok {
            "支持 Windows Portable 运行时"
        } else {
            "首版需要 Windows x64"
        }
        .to_string(),
    };

    vec![gpu_check, memory_check, system_check]
}

fn build_recommendations(tier: &CapabilityTier, total_memory: u64) -> Vec<String> {
    let mut recommendations = vec![
        "首次生成使用客户端推荐的分辨率与时长，成功后再提高参数。".to_string(),
        "生成期间关闭游戏、三维软件和其他占用显存的程序。".to_string(),
        "模型目录建议放在至少还剩 70 GB 的 SSD。".to_string(),
    ];
    if total_memory < 64 * GIB
        || matches!(
            tier,
            CapabilityTier::Experimental | CapabilityTier::HighRisk
        )
    {
        recommendations.push(
            "将 Windows 页面文件设为系统管理或至少 48 GB，避免模型卸载时内存不足。".to_string(),
        );
    }
    recommendations
}

#[cfg(test)]
mod tests {
    use super::*;

    fn nvidia(vram_mb: u64) -> Vec<GpuInfo> {
        vec![GpuInfo {
            name: "Test NVIDIA".into(),
            vendor: "NVIDIA".into(),
            vram_mb: Some(vram_mb),
            driver_version: Some("1".into()),
            compute_capability: Some("9.0".into()),
            cuda_available: true,
        }]
    }

    #[test]
    fn comfortable_requires_24gb_vram_and_64gb_ram() {
        let (tier, _, _, preset) = evaluate_tier(&nvidia(24 * 1024), 64 * GIB, "x86_64");
        assert_eq!(tier, CapabilityTier::Comfortable);
        assert_eq!(preset.width, 1344);
    }

    #[test]
    fn sixteen_gb_is_usable() {
        let (tier, _, _, _) = evaluate_tier(&nvidia(16 * 1024), 32 * GIB, "x86_64");
        assert_eq!(tier, CapabilityTier::Usable);
    }

    #[test]
    fn missing_cuda_is_unsupported() {
        let (tier, _, _, _) = evaluate_tier(&[], 64 * GIB, "x86_64");
        assert_eq!(tier, CapabilityTier::Unsupported);
    }
}

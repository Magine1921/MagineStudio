import { convertFileSrc, invoke } from "@tauri-apps/api/core";
import { open, save } from "@tauri-apps/plugin-dialog";
import type {
  AppSettings,
  ComfyStatus,
  EnvironmentReport,
  GenerationJob,
  GenerationRequest,
  HardwareReport,
  InstallPlan,
  InstallRequest,
  InstallationStatus,
} from "./types";
import { DEFAULT_SETTINGS } from "./types";

export const isTauriRuntime = () => "__TAURI_INTERNALS__" in window;

const SETTINGS_STORAGE_KEY = "maginestudio-settings";
const LEGACY_SETTINGS_STORAGE_KEY = "h3studio-settings";

const browserMockReport: HardwareReport = {
  scannedAt: new Date().toISOString(),
  osName: "Windows",
  osVersion: "11",
  architecture: "x86_64",
  cpuName: "桌面预览模式",
  physicalCores: 8,
  logicalCores: 16,
  totalMemoryBytes: 64 * 1024 ** 3,
  availableMemoryBytes: 42 * 1024 ** 3,
  gpus: [
    {
      name: "NVIDIA GeForce RTX 4090",
      vendor: "NVIDIA",
      vramMb: 24564,
      driverVersion: "preview",
      computeCapability: "8.9",
      cudaAvailable: true,
    },
  ],
  disks: [
    { name: "C:", mountPoint: "C:\\", totalBytes: 1024 ** 4, availableBytes: 410 * 1024 ** 3 },
  ],
  tier: "comfortable",
  tierLabel: "舒适运行",
  summary: "这台电脑适合从 768p、5 秒开始生成。",
  checks: [
    { id: "gpu", label: "显卡", value: "RTX 4090 · 24 GB", status: "pass", message: "CUDA 显卡可用" },
    { id: "memory", label: "内存", value: "64 GB", status: "pass", message: "满足舒适运行建议" },
    { id: "system", label: "系统", value: "Windows 11 x64", status: "pass", message: "支持一键安装" },
  ],
  recommendations: ["首个任务建议使用 1344 × 768、5 秒。", "生成期间关闭大型游戏和占用显存的软件。"],
  preset: { width: 1344, height: 768, durationSeconds: 5, steps: 20 },
  rulesVersion: "2026.08.1",
};

export async function scanHardware(): Promise<HardwareReport> {
  if (!isTauriRuntime()) return { ...browserMockReport, scannedAt: new Date().toISOString() };
  return invoke<HardwareReport>("scan_hardware");
}

export async function scanEnvironment(
  settings: AppSettings,
  includeReferenceModel = false,
): Promise<EnvironmentReport> {
  if (!isTauriRuntime()) {
    const pathSpaces = [
      { id: "comfyui" as const, label: "ComfyUI", path: settings.comfyPath, requiredBytes: 8 * 1024 ** 3 },
      { id: "models" as const, label: "模型", path: settings.modelPath, requiredBytes: includeReferenceModel ? 62 * 1024 ** 3 : 42 * 1024 ** 3 },
      { id: "output" as const, label: "输出", path: settings.outputPath, requiredBytes: 1024 ** 3 },
    ].map((item) => ({
      ...item,
      mountPoint: "C:\\",
      totalBytes: 1024 ** 4,
      availableBytes: 410 * 1024 ** 3,
      sufficient: true,
      message: "浏览器预览使用模拟磁盘信息。",
    }));
    const components = ["python", "git", "cuda", "comfyui", "models", "configuration"].map((id) => ({
      id,
      name: id,
      state: "missing" as const,
      version: null,
      path: null,
      message: "桌面客户端会执行真实检测。",
    }));
    return { scannedAt: new Date().toISOString(), components, pathSpaces, suggestedComfyPath: null, suggestedModelPath: null, alignmentMessage: null, readyCount: 0, totalCount: components.length, summary: "桌面客户端会执行真实检测。" };
  }
  return invoke<EnvironmentReport>("scan_environment", { settings, includeReferenceModel });
}

export async function loadSettings(): Promise<AppSettings> {
  if (!isTauriRuntime()) {
    const saved = localStorage.getItem(SETTINGS_STORAGE_KEY) ?? localStorage.getItem(LEGACY_SETTINGS_STORAGE_KEY);
    if (saved && !localStorage.getItem(SETTINGS_STORAGE_KEY)) {
      localStorage.setItem(SETTINGS_STORAGE_KEY, saved);
    }
    return saved ? { ...DEFAULT_SETTINGS, ...JSON.parse(saved) } : DEFAULT_SETTINGS;
  }
  return invoke<AppSettings>("load_settings");
}

export async function getDefaultSettings(): Promise<AppSettings> {
  if (!isTauriRuntime()) return DEFAULT_SETTINGS;
  return invoke<AppSettings>("get_default_settings");
}

export async function saveSettings(settings: AppSettings): Promise<void> {
  if (!isTauriRuntime()) {
    localStorage.setItem(SETTINGS_STORAGE_KEY, JSON.stringify(settings));
    return;
  }
  await invoke("save_settings", { settings });
}

export async function chooseDirectory(defaultPath: string): Promise<string | null> {
  if (!isTauriRuntime()) return defaultPath;
  const selected = await open({ directory: true, multiple: false, defaultPath });
  return typeof selected === "string" ? selected : null;
}

export async function chooseImages(multiple = false): Promise<string[]> {
  if (!isTauriRuntime()) return [];
  const selected = await open({
    directory: false,
    multiple,
    filters: [{ name: "图片", extensions: ["png", "jpg", "jpeg", "webp"] }],
  });
  if (typeof selected === "string") return [selected];
  return Array.isArray(selected) ? selected : [];
}

export async function chooseReferenceMedia(): Promise<string[]> {
  if (!isTauriRuntime()) return [];
  const selected = await open({
    directory: false,
    multiple: true,
    filters: [{
      name: "全模态参考素材",
      extensions: ["png", "jpg", "jpeg", "webp", "mp4", "mov", "mkv", "webm", "avi", "wav", "mp3", "flac", "m4a", "ogg", "aac"],
    }],
  });
  if (typeof selected === "string") return [selected];
  return Array.isArray(selected) ? selected : [];
}

export async function createInstallPlan(request: InstallRequest): Promise<InstallPlan> {
  if (!isTauriRuntime()) {
    const base = 2_067_155_290 + 20_970_379_616 + 15_687_142_551 + 605_254_808 + 5_207_808_496;
    const extra = request.includeReferenceModel ? 20_970_379_616 : 0;
    return {
      components: [],
      downloadBytes: base + extra,
      requiredFreeBytes: request.includeReferenceModel ? 80 * 1024 ** 3 : 55 * 1024 ** 3,
      warnings: [],
      modelFiles: [],
      comfyVersion: "0.31.0",
      licenseVersion: "2026-08-02",
    };
  }
  return invoke<InstallPlan>("create_install_plan", { request });
}

export async function startInstall(request: InstallRequest): Promise<InstallationStatus> {
  if (!isTauriRuntime()) throw new Error("浏览器预览模式不会下载约 42 GB 的组件，请在桌面客户端中执行安装。");
  return invoke<InstallationStatus>("start_install", { request });
}

export async function cancelInstall(): Promise<void> {
  if (isTauriRuntime()) await invoke("cancel_install");
}

export async function getInstallationStatus(settings: AppSettings): Promise<InstallationStatus> {
  if (!isTauriRuntime()) {
    return { installed: false, healthy: false, comfyVersion: null, installedModels: [], message: "尚未安装", runtimePath: null };
  }
  return invoke<InstallationStatus>("get_installation_status", { settings });
}

export async function startComfy(settings: AppSettings): Promise<ComfyStatus> {
  return invoke<ComfyStatus>("start_comfyui", { settings });
}

export async function stopComfy(): Promise<ComfyStatus> {
  return invoke<ComfyStatus>("stop_comfyui");
}

export async function getComfyStatus(): Promise<ComfyStatus> {
  if (!isTauriRuntime()) return { state: "stopped", message: "浏览器预览模式", pid: null, url: "http://127.0.0.1:8188" };
  return invoke<ComfyStatus>("get_comfy_status");
}

export async function queueGeneration(request: GenerationRequest, settings: AppSettings): Promise<GenerationJob> {
  return invoke<GenerationJob>("queue_generation", { request, settings });
}

export async function pollGeneration(promptId: string, clientId: string, settings: AppSettings): Promise<GenerationJob> {
  return invoke<GenerationJob>("poll_generation", { promptId, clientId, settings });
}

export function generationVideoSource(job: GenerationJob | null): string | null {
  if (!job) return null;
  if (job.localVideoPath && isTauriRuntime()) return convertFileSrc(job.localVideoPath);
  return job.videoUrl;
}

export async function cancelGeneration(): Promise<void> {
  await invoke("cancel_generation");
}

export async function openExternal(url: string): Promise<void> {
  if (!isTauriRuntime()) {
    window.open(url, "_blank", "noopener,noreferrer");
    return;
  }
  await invoke("open_external", { url });
}

export async function openOutputFolder(path: string): Promise<void> {
  if (isTauriRuntime()) await invoke("open_output_folder", { path });
}

export async function exportTextFile(defaultName: string, contents: string): Promise<string | null> {
  if (!isTauriRuntime()) {
    const blob = new Blob([contents], { type: "text/plain;charset=utf-8" });
    const url = URL.createObjectURL(blob);
    const anchor = document.createElement("a");
    anchor.href = url;
    anchor.download = defaultName;
    anchor.click();
    URL.revokeObjectURL(url);
    return defaultName;
  }
  const selected = await save({
    defaultPath: defaultName,
    filters: [{ name: "文本文件", extensions: ["txt", "json", "md", "log"] }],
  });
  if (!selected) return null;
  await invoke("write_text_file", { path: selected, contents });
  return selected;
}

export async function clearCache(path: string): Promise<void> {
  if (!isTauriRuntime()) return;
  await invoke("clear_cache", { path });
}

export type CapabilityTier = "comfortable" | "usable" | "experimental" | "high_risk" | "unsupported";

export interface GpuInfo {
  name: string;
  vendor: string;
  vramMb: number | null;
  driverVersion: string | null;
  computeCapability: string | null;
  cudaAvailable: boolean;
}

export interface DiskInfo {
  name: string;
  mountPoint: string;
  totalBytes: number;
  availableBytes: number;
}

export interface EnvironmentComponentStatus {
  id: string;
  name: string;
  state: "ready" | "partial" | "missing";
  version: string | null;
  path: string | null;
  message: string;
}

export interface PathSpaceInfo {
  id: "comfyui" | "models" | "output";
  label: string;
  path: string;
  mountPoint: string | null;
  totalBytes: number | null;
  availableBytes: number | null;
  requiredBytes: number;
  sufficient: boolean;
  message: string;
}

export interface EnvironmentReport {
  scannedAt: string;
  components: EnvironmentComponentStatus[];
  pathSpaces: PathSpaceInfo[];
  suggestedComfyPath: string | null;
  suggestedModelPath: string | null;
  alignmentMessage: string | null;
  readyCount: number;
  totalCount: number;
  summary: string;
}

export interface HardwareCheck {
  id: string;
  label: string;
  value: string;
  status: "pass" | "warning" | "fail" | "unknown";
  message: string;
}

export interface GenerationPreset {
  width: number;
  height: number;
  durationSeconds: number;
  steps: number;
}

export interface HardwareReport {
  scannedAt: string;
  osName: string;
  osVersion: string;
  architecture: string;
  cpuName: string;
  physicalCores: number | null;
  logicalCores: number;
  totalMemoryBytes: number;
  availableMemoryBytes: number;
  gpus: GpuInfo[];
  disks: DiskInfo[];
  tier: CapabilityTier;
  tierLabel: string;
  summary: string;
  checks: HardwareCheck[];
  recommendations: string[];
  preset: GenerationPreset;
  rulesVersion: string;
}

export interface AppSettings {
  comfyPath: string;
  modelPath: string;
  outputPath: string;
  enabledModels: string[] | null;
  licenseAccepted: boolean;
  regionConfirmed: boolean;
  acceptedLicenseVersion: string | null;
  acceptedAt: string | null;
}

export interface InstallComponent {
  id: string;
  name: string;
  description: string;
  bytes: number;
  required: boolean;
  selected: boolean;
}

export interface InstallPlan {
  components: InstallComponent[];
  downloadBytes: number;
  requiredFreeBytes: number;
  warnings: string[];
  modelFiles: string[];
  comfyVersion: string;
  licenseVersion: string;
}

export interface InstallRequest {
  comfyPath: string;
  modelPath: string;
  outputPath: string;
  includeReferenceModel: boolean;
  licenseAccepted: boolean;
  regionConfirmed: boolean;
  licenseVersion: string;
}

export interface InstallProgress {
  stage: string;
  stageLabel: string;
  fileName: string | null;
  fileDownloadedBytes: number;
  fileTotalBytes: number;
  totalDownloadedBytes: number;
  totalBytes: number;
  percent: number;
  message: string;
  canCancel: boolean;
}

export interface InstallationStatus {
  installed: boolean;
  healthy: boolean;
  comfyVersion: string | null;
  installedModels: string[];
  message: string;
  runtimePath: string | null;
}

export interface ComfyStatus {
  state: "stopped" | "starting" | "ready" | "error";
  message: string;
  pid: number | null;
  url: string;
}

export interface GenerationRequest {
  prompt: string;
  selectedModel: string;
  width: number;
  height: number;
  durationSeconds: number;
  fps: number;
  steps: number;
  seed: number;
  firstFramePath: string | null;
  lastFramePath: string | null;
  referencePaths: string[];
  outputPrefix: string;
}

export interface GenerationJob {
  promptId: string;
  clientId: string;
  state: "queued" | "running" | "completed" | "failed";
  message: string;
  videoUrl: string | null;
  localVideoPath: string | null;
  filename: string | null;
  progressPercent?: number;
  currentStep?: number | null;
  totalSteps?: number | null;
  elapsedSeconds?: number;
  estimatedRemainingSeconds?: number | null;
  startedAt?: string;
  selectedModel?: string;
  width?: number;
  height?: number;
  durationSeconds?: number;
  fps?: number;
}

export const DEFAULT_SETTINGS: AppSettings = {
  comfyPath: "C:\\MagineStudio\\ComfyUI",
  modelPath: "C:\\MagineStudio\\Models",
  outputPath: "C:\\MagineStudio\\Outputs",
  enabledModels: null,
  licenseAccepted: false,
  regionConfirmed: false,
  acceptedLicenseVersion: null,
  acceptedAt: null,
};

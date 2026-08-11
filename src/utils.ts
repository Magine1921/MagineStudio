import type { CapabilityTier, GenerationJob } from "./types";

export function formatBytes(bytes: number, digits = 1): string {
  if (!Number.isFinite(bytes) || bytes <= 0) return "0 B";
  const units = ["B", "KB", "MB", "GB", "TB"];
  const index = Math.min(Math.floor(Math.log(bytes) / Math.log(1024)), units.length - 1);
  return `${(bytes / 1024 ** index).toFixed(index === 0 ? 0 : digits)} ${units[index]}`;
}

export function formatDate(iso: string): string {
  return new Intl.DateTimeFormat("zh-CN", {
    month: "2-digit",
    day: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
  }).format(new Date(iso));
}

export function errorMessage(error: unknown): string {
  if (error instanceof Error) return error.message;
  if (typeof error === "string") return error;
  return "发生未知错误，请重试。";
}

export const tierTone: Record<CapabilityTier, "success" | "info" | "warning" | "danger"> = {
  comfortable: "success",
  usable: "info",
  experimental: "warning",
  high_risk: "warning",
  unsupported: "danger",
};

export function basename(path: string): string {
  return path.split(/[\\/]/).filter(Boolean).at(-1) ?? path;
}

export function mergeGenerationJob(
  current: GenerationJob | null,
  next: GenerationJob,
): GenerationJob {
  const sameJob = current?.promptId === next.promptId;
  const tracked = sameJob ? {
    startedAt: current.startedAt,
    elapsedSeconds: current.elapsedSeconds,
    estimatedRemainingSeconds: current.estimatedRemainingSeconds,
    currentStep: current.currentStep,
    totalSteps: current.totalSteps,
    progressPercent: current.progressPercent,
    selectedModel: current.selectedModel,
    width: current.width,
    height: current.height,
    durationSeconds: current.durationSeconds,
    fps: current.fps,
  } : {};
  return {
    ...next,
    ...tracked,
    progressPercent: next.state === "completed"
      ? 100
      : next.state === "failed"
        ? sameJob ? current?.progressPercent ?? 0 : next.progressPercent ?? 0
        : sameJob ? current?.progressPercent ?? next.progressPercent ?? 0 : next.progressPercent ?? 0,
    estimatedRemainingSeconds: next.state === "completed"
      ? 0
      : tracked.estimatedRemainingSeconds,
  };
}

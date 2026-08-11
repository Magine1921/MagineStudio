import { describe, expect, it } from "vitest";
import type { GenerationJob } from "./types";
import { basename, formatBytes, mergeGenerationJob } from "./utils";

describe("formatBytes", () => {
  it("formats the base model download size in GB", () => {
    expect(formatBytes(44_537_740_761)).toBe("41.5 GB");
  });
});

describe("basename", () => {
  it("supports Windows and Unix separators", () => {
    expect(basename("D:\\images\\first.png")).toBe("first.png");
    expect(basename("/tmp/first.png")).toBe("first.png");
  });
});

describe("mergeGenerationJob", () => {
  const running: GenerationJob = {
    promptId: "job-1",
    clientId: "client-1",
    state: "running",
    message: "running",
    videoUrl: null,
    localVideoPath: null,
    filename: null,
    progressPercent: 64,
    elapsedSeconds: 120,
    estimatedRemainingSeconds: 60,
    startedAt: "2026-08-11T00:00:00Z",
    selectedModel: "G:\\models\\minimax_h3_fl2va.safetensors",
    width: 608,
    height: 352,
    durationSeconds: 4,
    fps: 24,
  };

  it("marks a completed backend job as 100% while preserving timing", () => {
    const completed = mergeGenerationJob(running, {
      ...running,
      state: "completed",
      message: "done",
      localVideoPath: "F:\\output\\video.mp4",
    });
    expect(completed.progressPercent).toBe(100);
    expect(completed.estimatedRemainingSeconds).toBe(0);
    expect(completed.elapsedSeconds).toBe(120);
    expect(completed.width).toBe(608);
  });

  it("does not reuse tracking data from another task", () => {
    const next = mergeGenerationJob(running, {
      ...running,
      promptId: "job-2",
      progressPercent: 5,
      elapsedSeconds: 1,
    });
    expect(next.progressPercent).toBe(5);
    expect(next.elapsedSeconds).toBe(1);
  });
});

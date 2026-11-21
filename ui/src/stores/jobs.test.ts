import { toast } from "sonner";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { setApiForTests } from "@/lib/ipc";
import { createMockApi } from "@/mock/api";
import type { JobProgress } from "@/lib/types";
import { activeJobs, useJobs } from "@/stores/jobs";

/** A running master export of track 3, `done` of 100. */
function masterAt(done: number): JobProgress {
  return {
    jobId: "m1",
    kind: "master",
    label: "Mastering a track",
    stage: null,
    trackIds: [3],
    done,
    total: 100,
    queued: false,
  };
}

describe("jobs store", () => {
  beforeEach(() => {
    setApiForTests(createMockApi());
    useJobs.setState({ jobs: {}, paused: false });
    vi.useFakeTimers();
  });

  afterEach(() => {
    vi.useRealTimers();
    vi.restoreAllMocks();
  });

  it("keeps a master export running while the queue is paused (it runs beside it)", async () => {
    useJobs.getState().onProgress(masterAt(10));
    await useJobs.getState().setPaused(true);
    expect(useJobs.getState().jobs["m1"]?.state).toBe("running");
    useJobs.getState().onProgress(masterAt(20));
    expect(useJobs.getState().jobs["m1"]?.state).toBe("running");
    await useJobs.getState().setPaused(false);
    expect(useJobs.getState().jobs["m1"]?.state).toBe("running");
  });

  it("keeps the model download running while the queue is paused", async () => {
    await useJobs.getState().setPaused(true);
    useJobs.getState().onProgress({
      ...masterAt(10),
      jobId: "d1",
      kind: "model-download",
      stage: "download",
      trackIds: [],
    });
    expect(useJobs.getState().jobs["d1"]?.state).toBe("running");
  });

  it("leaves a failed master export's toast to the Master tab, and toasts other jobs", () => {
    const error = vi.spyOn(toast, "error");
    const { onProgress, onFailed } = useJobs.getState();
    onProgress(masterAt(40));
    onFailed({
      jobId: "m1",
      kind: "master",
      error: { code: "io", message: "disk full" },
    });
    expect(useJobs.getState().jobs["m1"]?.state).toBe("failed");
    expect(error).not.toHaveBeenCalled();
    onProgress({ ...masterAt(1), jobId: "a1", kind: "analysis" });
    onFailed({ jobId: "a1", kind: "analysis", error: { code: "io", message: "x" } });
    expect(error).toHaveBeenCalledTimes(1);
  });

  it("tracks progress, then drops finished jobs after a short delay", () => {
    const { onProgress, onDone } = useJobs.getState();
    onProgress({
      jobId: "j1",
      kind: "analysis",
      label: "Analyzing",
      stage: null,
      trackIds: [],
      done: 1,
      total: 4,
      queued: false,
    });
    expect(activeJobs(useJobs.getState().jobs)).toHaveLength(1);
    onDone({ jobId: "j1", kind: "analysis" });
    expect(useJobs.getState().jobs["j1"]?.state).toBe("done");
    expect(activeJobs(useJobs.getState().jobs)).toHaveLength(0);
    vi.advanceTimersByTime(4000);
    expect(useJobs.getState().jobs["j1"]).toBeUndefined();
  });

  it("shows a queued job as queued until it starts, and it can be cancelled", async () => {
    const { onProgress } = useJobs.getState();
    const queued: JobProgress = {
      jobId: "j9",
      kind: "analysis",
      label: "Analyzing 1 tracks",
      stage: null,
      trackIds: [3],
      done: 0,
      total: 100,
      queued: true,
    };
    onProgress(queued);
    expect(useJobs.getState().jobs["j9"]?.state).toBe("queued");
    expect(activeJobs(useJobs.getState().jobs)).toHaveLength(1);
    onProgress({ ...queued, queued: false, done: 40 });
    expect(useJobs.getState().jobs["j9"]?.state).toBe("running");
    await useJobs.getState().cancel("j9");
    expect(useJobs.getState().jobs["j9"]).toBeUndefined();
  });

  it("pauses and resumes running jobs", async () => {
    useJobs.getState().onProgress({
      jobId: "j2",
      kind: "stems",
      label: "Splitting",
      stage: null,
      trackIds: [],
      done: 2,
      total: 20,
      queued: false,
    });
    await useJobs.getState().setPaused(true);
    expect(useJobs.getState().jobs["j2"]?.state).toBe("paused");
    await useJobs.getState().setPaused(false);
    expect(useJobs.getState().jobs["j2"]?.state).toBe("running");
  });

  it("keeps a cancelled job gone when late progress arrives", async () => {
    const { onProgress } = useJobs.getState();
    onProgress({
      jobId: "j3",
      kind: "analysis",
      label: "Analyzing",
      stage: null,
      trackIds: [],
      done: 1,
      total: 9,
      queued: false,
    });
    await useJobs.getState().cancel("j3");
    onProgress({
      jobId: "j3",
      kind: "analysis",
      label: "Analyzing",
      stage: null,
      trackIds: [],
      done: 2,
      total: 9,
      queued: false,
    });
    useJobs.getState().onDone({ jobId: "j3", kind: "analysis" });
    expect(useJobs.getState().jobs["j3"]).toBeUndefined();
  });
});

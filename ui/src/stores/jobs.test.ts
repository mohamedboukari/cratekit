import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { setApiForTests } from "@/lib/ipc";
import { createMockApi } from "@/mock/api";
import type { JobProgress } from "@/lib/types";
import { activeJobs, useJobs } from "@/stores/jobs";

describe("jobs store", () => {
  beforeEach(() => {
    setApiForTests(createMockApi());
    useJobs.setState({ jobs: {}, paused: false });
    vi.useFakeTimers();
  });

  afterEach(() => {
    vi.useRealTimers();
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

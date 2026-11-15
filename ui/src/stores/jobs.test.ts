import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { setApiForTests } from "@/lib/ipc";
import { createMockApi } from "@/mock/api";
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
    onProgress({ jobId: "j1", kind: "analysis", label: "Analyzing", done: 1, total: 4 });
    expect(activeJobs(useJobs.getState().jobs)).toHaveLength(1);
    onDone({ jobId: "j1", kind: "analysis" });
    expect(useJobs.getState().jobs["j1"]?.state).toBe("done");
    expect(activeJobs(useJobs.getState().jobs)).toHaveLength(0);
    vi.advanceTimersByTime(4000);
    expect(useJobs.getState().jobs["j1"]).toBeUndefined();
  });

  it("pauses and resumes running jobs", async () => {
    useJobs
      .getState()
      .onProgress({ jobId: "j2", kind: "stems", label: "Splitting", done: 2, total: 20 });
    await useJobs.getState().setPaused(true);
    expect(useJobs.getState().jobs["j2"]?.state).toBe("paused");
    await useJobs.getState().setPaused(false);
    expect(useJobs.getState().jobs["j2"]?.state).toBe("running");
  });
});

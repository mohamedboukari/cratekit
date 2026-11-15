import { create } from "zustand";

import { api } from "@/lib/ipc";
import { notifyError } from "@/lib/notify";
import type { Job, JobDone, JobFailed, JobId, JobProgress, JobState } from "@/lib/types";

/** Finished jobs stay visible this long in the queue before they are dropped. */
const DONE_TTL_MS = 4000;

/** Jobs the user cancelled: late progress or done events for them are ignored. */
const cancelled = new Set<JobId>();

interface JobsState {
  jobs: Record<JobId, Job>;
  paused: boolean;
  /** Coarse text for screen readers: a job started or finished (never progress ticks). */
  announcement: string;
  onProgress: (p: JobProgress) => void;
  onDone: (d: JobDone) => void;
  onFailed: (f: JobFailed) => void;
  cancel: (id: JobId) => Promise<void>;
  setPaused: (paused: boolean) => Promise<void>;
}

/** Jobs that are not finished, oldest first. */
export function activeJobs(jobs: Record<JobId, Job>): Job[] {
  return Object.values(jobs).filter(
    (j) => j.state === "running" || j.state === "queued" || j.state === "paused",
  );
}

export const useJobs = create<JobsState>()((set, get) => ({
  jobs: {},
  paused: false,
  announcement: "",

  onProgress: (p) => {
    if (cancelled.has(p.jobId)) return;
    set((s) => ({
      announcement: s.jobs[p.jobId] ? s.announcement : `${p.label} started`,
      jobs: {
        ...s.jobs,
        [p.jobId]: {
          id: p.jobId,
          kind: p.kind,
          label: p.label,
          done: p.done,
          total: p.total,
          state: s.paused ? "paused" : "running",
        },
      },
    }));
  },

  onDone: (d) => {
    if (cancelled.has(d.jobId)) return;
    set((s) => {
      const job = s.jobs[d.jobId];
      if (!job) return s;
      return {
        announcement: `${job.label} done`,
        jobs: { ...s.jobs, [d.jobId]: { ...job, done: job.total, state: "done" } },
      };
    });
    setTimeout(() => {
      set((s) => {
        const { [d.jobId]: _finished, ...rest } = s.jobs;
        return { jobs: rest };
      });
    }, DONE_TTL_MS);
  },

  onFailed: (f) => {
    set((s) => {
      const job = s.jobs[f.jobId];
      if (!job) return s;
      return { jobs: { ...s.jobs, [f.jobId]: { ...job, state: "failed" } } };
    });
    notifyError(f.error, "A background job failed");
  },

  cancel: async (id) => {
    try {
      await api().jobCancel(id);
      cancelled.add(id);
      set((s) => {
        const { [id]: _cancelled, ...rest } = s.jobs;
        return { jobs: rest };
      });
    } catch (error) {
      notifyError(error, "Could not cancel the job");
    }
  },

  setPaused: async (paused) => {
    try {
      await api().jobsSetPaused(paused);
      const jobs: Record<JobId, Job> = {};
      for (const [id, job] of Object.entries(get().jobs)) {
        const toggles = job.state === "running" || job.state === "paused";
        const state: JobState = toggles ? (paused ? "paused" : "running") : job.state;
        jobs[id] = { ...job, state };
      }
      set({ paused, jobs });
    } catch (error) {
      notifyError(error, paused ? "Could not pause jobs" : "Could not resume jobs");
    }
  },
}));

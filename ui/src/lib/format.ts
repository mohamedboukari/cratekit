/** Display formatting only (no domain logic). */
import { i18n } from "@/i18n";
import type { Job } from "@/lib/types";

/** 0:00 style time; negative values show as 0:00. */
export function formatTime(sec: number): string {
  const s = Math.max(0, Math.floor(sec));
  return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, "0")}`;
}

/** "1 h 12 min" for a total duration in seconds, in the UI language. */
export function formatDuration(sec: number): string {
  const minutes = Math.round(sec / 60);
  const h = Math.floor(minutes / 60);
  const m = minutes % 60;
  return h > 0
    ? i18n.t("common.format.durationHours", { h, m })
    : i18n.t("common.format.durationMinutes", { m });
}

/** Filter knob text: "Off", "Low-pass 40%" or "High-pass 25%", in the UI language. */
export function filterText(value: number): string {
  const pct = Math.round(Math.abs(value) * 100);
  if (pct < 2) return i18n.t("common.format.filterOff");
  return value < 0
    ? i18n.t("common.format.lowPass", { pct })
    : i18n.t("common.format.highPass", { pct });
}

/** Status text for the stems backend Rust reports: "GPU: CoreML", "GPU: DirectML" or "CPU". */
export function backendLabel(active: "coreml" | "directml" | "cpu"): string {
  switch (active) {
    case "coreml":
      return i18n.t("common.format.backend.coreml");
    case "directml":
      return i18n.t("common.format.backend.directml");
    case "cpu":
      return i18n.t("common.format.backend.cpu");
  }
}

/**
 * What a background job is doing, in the UI language. Built from the job's
 * kind, track count and stage, so it never shows Rust's English label.
 */
export function jobLabel(job: {
  kind: Job["kind"];
  /** Only read, so a read-only list is fine. */
  trackIds: readonly number[];
  stage: Job["stage"];
}): string {
  switch (job.kind) {
    case "analysis":
      return i18n.t("common.jobs.kinds.analysis", { count: job.trackIds.length });
    case "stems":
      return i18n.t("common.jobs.kinds.stems");
    case "master":
      return i18n.t("common.jobs.kinds.master");
    case "export":
      return i18n.t("common.jobs.kinds.export");
    case "model-download":
      return job.stage === "verify"
        ? i18n.t("common.jobs.kinds.modelVerify")
        : i18n.t("common.jobs.kinds.modelDownload");
  }
}

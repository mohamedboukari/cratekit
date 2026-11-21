import { create } from "zustand";

import { i18n } from "@/i18n";
import { api, toAppError } from "@/lib/ipc";
import { notifyError } from "@/lib/notify";
import type {
  JobId,
  MasterPreset,
  MasterResult,
  MasterSettings,
  PresetName,
  TrackId,
} from "@/lib/types";
import { useJobs } from "@/stores/jobs";
import { usePlayer } from "@/stores/player";
import { refreshAfterAnalysis } from "@/stores/refresh";
import { useUi } from "@/stores/ui";

/** Settings changes are sent to Rust for the B side after a short pause. */
const PREVIEW_DELAY_MS = 250;

/** A = original, B = mastered. */
export type AbSide = "a" | "b";

interface MasterState {
  presets: MasterPreset[];
  presetsStatus: "loading" | "ready" | "error";
  settings: MasterSettings;
  /** Title of the reference track, for display (the settings hold its id). */
  referenceTitle: string | null;
  side: AbSide;
  levelMatch: boolean;
  /** An export is running (a `master` job: its progress lives in the jobs store). */
  exporting: boolean;
  /** Cancel was pressed; the export has not stopped yet (Rust checks between passes). */
  cancelling: boolean;
  /** Title of the track being exported, for when another track is on the deck. */
  exportTitle: string | null;
  /** The last export, and the track it belongs to. */
  result: { trackId: TrackId; master: MasterResult } | null;
  loadPresets: () => Promise<void>;
  setPreset: (name: PresetName) => void;
  setIntensity: (value: number) => void;
  setTargetLufs: (value: number) => void;
  setReference: (track: { id: TrackId; title: string } | null) => void;
  setBitDepth: (depth: 16 | 24) => void;
  setSide: (side: AbSide) => void;
  toggleSide: () => void;
  setLevelMatch: (enabled: boolean) => Promise<void>;
  /** Sends Rust the current settings for this track's B side (mastered live as it plays). */
  preview: (id: TrackId) => Promise<void>;
  /** Exports the master; null when it failed (shown as a toast) or was cancelled (silent). */
  exportMaster: (id: TrackId) => Promise<MasterResult | null>;
  /** Cancels the running export (its `master` job). */
  cancelExport: (jobId: JobId) => Promise<void>;
}

let previewTimer: ReturnType<typeof setTimeout> | null = null;

export const useMaster = create<MasterState>()((set, get) => {
  /**
   * Sends the B side's settings after a change, once the user pauses, and
   * only while the B side is what you hear (Master tab, side B). The track is
   * read when the timer fires, so a track switch never previews the old one.
   */
  function schedulePreview(): void {
    if (previewTimer !== null) clearTimeout(previewTimer);
    previewTimer = setTimeout(() => {
      previewTimer = null;
      const id = usePlayer.getState().detail?.track.id;
      const hearingB = useUi.getState().activeTool === "master" && get().side === "b";
      if (id !== undefined && hearingB) void get().preview(id);
    }, PREVIEW_DELAY_MS);
  }

  function patch(changes: Partial<MasterSettings>): void {
    set({ settings: { ...get().settings, ...changes } });
    schedulePreview();
  }

  return {
    presets: [],
    presetsStatus: "loading",
    settings: {
      preset: "club",
      intensity: 0.6,
      targetLufs: -8,
      referenceTrackId: null,
      bitDepth: 24,
    },
    referenceTitle: null,
    side: "b",
    levelMatch: true,
    exporting: false,
    cancelling: false,
    exportTitle: null,
    result: null,

    loadPresets: async () => {
      set({ presetsStatus: "loading" });
      try {
        const presets = await api().masterPresets();
        const current = presets.find((p) => p.name === get().settings.preset);
        set({ presets, presetsStatus: "ready" });
        if (current)
          set({ settings: { ...get().settings, targetLufs: current.targetLufs } });
      } catch (error) {
        set({ presetsStatus: "error" });
        notifyError(error, i18n.t("errors.master.presets"));
      }
    },

    // Picking a preset also picks its loudness target (Rust's default for it).
    setPreset: (name) => {
      const preset = get().presets.find((p) => p.name === name);
      patch(preset ? { preset: name, targetLufs: preset.targetLufs } : { preset: name });
    },
    setIntensity: (intensity) =>
      patch({ intensity: Math.max(0, Math.min(1, intensity)) }),
    setTargetLufs: (targetLufs) => patch({ targetLufs }),
    setReference: (track) => {
      set({ referenceTitle: track?.title ?? null });
      patch({ referenceTrackId: track?.id ?? null });
    },
    setBitDepth: (bitDepth) => set({ settings: { ...get().settings, bitDepth } }),
    setSide: (side) => set({ side }),
    toggleSide: () => set({ side: get().side === "a" ? "b" : "a" }),

    setLevelMatch: async (enabled) => {
      const previous = get().levelMatch;
      set({ levelMatch: enabled });
      try {
        await api().playbackSetLevelMatch(enabled);
      } catch (error) {
        set({ levelMatch: previous });
        notifyError(error, i18n.t("errors.master.levelMatch"));
      }
    },

    preview: async (id) => {
      try {
        await api().masterPreview(id, get().settings);
      } catch (error) {
        notifyError(error, i18n.t("errors.master.preview"));
      }
    },

    exportMaster: async (id) => {
      const detail = usePlayer.getState().detail;
      set({
        exporting: true,
        cancelling: false,
        exportTitle: detail?.track.id === id ? detail.track.title : null,
      });
      try {
        const master = await api().masterExport(id, get().settings);
        set({ result: { trackId: id, master } });
        // The track now shows as mastered in the browser and the set list.
        refreshAfterAnalysis();
        return master;
      } catch (error) {
        // Cancelled from the Master tab or the status bar: nothing went wrong.
        if (toAppError(error).code !== "cancelled")
          notifyError(error, i18n.t("errors.master.export"));
        return null;
      } finally {
        set({ exporting: false, cancelling: false, exportTitle: null });
      }
    },

    cancelExport: async (jobId) => {
      set({ cancelling: true });
      await useJobs.getState().cancel(jobId);
      // The cancel failed (shown as a toast): the export runs on.
      if (useJobs.getState().jobs[jobId]) set({ cancelling: false });
    },
  };
});

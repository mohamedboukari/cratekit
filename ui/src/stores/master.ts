import { create } from "zustand";

import { api } from "@/lib/ipc";
import { notifyError } from "@/lib/notify";
import type {
  MasterPreset,
  MasterResult,
  MasterSettings,
  PresetName,
  TrackId,
} from "@/lib/types";
import { usePlayer } from "@/stores/player";
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
  exporting: boolean;
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
  /** Asks Rust to render the B side for this track with the current settings. */
  preview: (id: TrackId) => Promise<void>;
  exportMaster: (id: TrackId) => Promise<MasterResult | null>;
}

let previewTimer: ReturnType<typeof setTimeout> | null = null;

export const useMaster = create<MasterState>()((set, get) => {
  /**
   * Re-renders the B side after a settings change, once the user pauses, and
   * only while the B side is what you hear (Master tab, side B). The track is
   * read when the timer fires, so a track switch never renders the old one.
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
        notifyError(error, "Could not load the mastering presets");
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
        notifyError(error, "Could not change level matching");
      }
    },

    preview: async (id) => {
      try {
        await api().masterPreview(id, get().settings);
      } catch (error) {
        notifyError(error, "Could not prepare the mastered preview");
      }
    },

    exportMaster: async (id) => {
      set({ exporting: true });
      try {
        const master = await api().masterExport(id, get().settings);
        set({ result: { trackId: id, master } });
        return master;
      } catch (error) {
        notifyError(error, "Could not export the master");
        return null;
      } finally {
        set({ exporting: false });
      }
    },
  };
});

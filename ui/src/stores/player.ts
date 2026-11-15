import { create } from "zustand";

import { api } from "@/lib/ipc";
import { notifyError } from "@/lib/notify";
import type { CueSlot, PlaybackMode, TrackDetail, TrackId } from "@/lib/types";

/** Only the newest load may update the deck or the engine (clicks can overlap). */
let loadSeq = 0;

/**
 * Deck state that changes at human speed. The playhead position and meters
 * arrive at about 30 Hz and live in refs (hooks/usePlaybackRefs.ts), not here.
 * Actions that need the current position take it as an argument.
 */
interface PlayerState {
  detail: TrackDetail | null;
  playing: boolean;
  mode: PlaybackMode;
  /** DJ filter: -1 low-pass, 0 off, +1 high-pass. */
  filter: number;
  load: (id: TrackId) => Promise<void>;
  togglePlay: () => Promise<void>;
  seek: (sec: number) => Promise<void>;
  setMode: (mode: PlaybackMode) => Promise<void>;
  setFilter: (value: number) => Promise<void>;
  /** CUE: back to the first beat and pause, ready to start on the beat. */
  cue: () => Promise<void>;
  /** Jumps to a hot cue, or sets it at `positionSec` when the slot is empty. */
  triggerHotCue: (slot: CueSlot, positionSec: number) => Promise<void>;
  /** Sets or moves a hot cue (e.g. after dragging its flag). */
  setHotCue: (slot: CueSlot, positionSec: number) => Promise<void>;
  clearHotCue: (slot: CueSlot) => Promise<void>;
  /** Rust reports a playback state change (e.g. the track reached its end). */
  onPlaybackState: (playing: boolean) => void;
}

export const usePlayer = create<PlayerState>()((set, get) => {
  /** Re-reads the loaded track (cues changed in Rust). */
  async function refreshDetail(): Promise<void> {
    const id = get().detail?.track.id;
    if (id === undefined) return;
    const detail = await api().trackDetail(id);
    if (get().detail?.track.id === id) set({ detail });
  }

  return {
    detail: null,
    playing: false,
    mode: "original",
    filter: 0,

    load: async (id) => {
      const seq = ++loadSeq;
      try {
        const detail = await api().trackDetail(id);
        if (seq !== loadSeq) return;
        await api().playbackLoad(id);
        if (seq !== loadSeq) return;
        set({ detail, playing: false });
      } catch (error) {
        if (seq !== loadSeq) return;
        notifyError(error, "Could not load the track");
      }
    },

    togglePlay: async () => {
      const { detail, playing } = get();
      if (!detail) return;
      try {
        if (playing) await api().playbackPause();
        else await api().playbackPlay();
        set({ playing: !playing });
      } catch (error) {
        notifyError(error, "Playback failed");
      }
    },

    seek: async (sec) => {
      try {
        await api().playbackSeek(sec);
      } catch (error) {
        notifyError(error, "Could not seek");
      }
    },

    setMode: async (mode) => {
      try {
        await api().playbackSetMode(mode);
        set({ mode });
      } catch (error) {
        notifyError(error, "Could not switch what you hear");
      }
    },

    setFilter: async (value) => {
      const clamped = Math.max(-1, Math.min(1, value));
      const previous = get().filter;
      set({ filter: clamped });
      try {
        await api().playbackSetFilter(clamped);
      } catch (error) {
        set({ filter: previous });
        notifyError(error, "Could not change the filter");
      }
    },

    cue: async () => {
      const detail = get().detail;
      if (!detail) return;
      try {
        if (get().playing) {
          await api().playbackPause();
          set({ playing: false });
        }
        await api().playbackSeek(detail.analysis?.firstBeatSec ?? 0);
      } catch (error) {
        notifyError(error, "Could not return to the cue point");
      }
    },

    triggerHotCue: async (slot, positionSec) => {
      const detail = get().detail;
      if (!detail) return;
      const existing = detail.cues.find((c) => c.slot === slot);
      if (existing) {
        await get().seek(existing.positionSec);
        return;
      }
      await get().setHotCue(slot, positionSec);
    },

    setHotCue: async (slot, positionSec) => {
      const detail = get().detail;
      if (!detail) return;
      try {
        await api().hotcueSet(detail.track.id, slot, positionSec);
        await refreshDetail();
      } catch (error) {
        notifyError(error, `Could not set hot cue ${slot}`);
      }
    },

    clearHotCue: async (slot) => {
      const detail = get().detail;
      if (!detail) return;
      try {
        await api().hotcueClear(detail.track.id, slot);
        await refreshDetail();
      } catch (error) {
        notifyError(error, `Could not clear hot cue ${slot}`);
      }
    },

    onPlaybackState: (playing) => set({ playing }),
  };
});

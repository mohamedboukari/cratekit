import { create } from "zustand";

import { i18n } from "@/i18n";
import { api } from "@/lib/ipc";
import { notifyError } from "@/lib/notify";
import type {
  CueColor,
  CueSlot,
  PlaybackMode,
  PlaybackState,
  TrackDetail,
  TrackId,
} from "@/lib/types";
import { useSetList } from "@/stores/setlist";

/** Only the newest load may update the deck or the engine (clicks can overlap). */
let loadSeq = 0;
/** Only the newest mode request may roll back on failure. */
let modeSeq = 0;
/**
 * Filter knob: one call to Rust at a time. A drag sends many values; while
 * a call is in flight the store only keeps the newest, and the running loop
 * sends it when the call returns, so Rust always ends on the last value.
 */
let filterSending = false;
/** The last filter value Rust accepted (where a failure rolls back to). */
let filterAccepted = 0;

/**
 * Deck state that changes at human speed. The playhead position and meters
 * arrive at about 30 Hz and live in refs (hooks/usePlaybackRefs.ts), not here.
 * Actions that need the current position take it as an argument.
 */
interface PlayerState {
  detail: TrackDetail | null;
  /** Counts successful loads (loading the same track again is a new load too). */
  loadCount: number;
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
  /** Renames or recolours a hot cue. */
  updateHotCue: (slot: CueSlot, name: string | null, color: CueColor) => Promise<void>;
  /**
   * Rust reports a playback state change. When the track reached its end and
   * it is in the set list, the next one loads and plays (no blend; blends
   * need the second deck of v0.2).
   */
  onPlaybackState: (state: PlaybackState) => void;
  /** Re-reads the loaded track (after a job changed its analysis or stems). */
  refresh: () => Promise<void>;
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
    refresh: async () => {
      try {
        await refreshDetail();
      } catch (error) {
        notifyError(error, i18n.t("errors.player.refresh"));
      }
    },
    detail: null,
    loadCount: 0,
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
        set({ detail, playing: false, loadCount: get().loadCount + 1 });
      } catch (error) {
        if (seq !== loadSeq) return;
        notifyError(error, i18n.t("errors.player.load"));
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
        notifyError(error, i18n.t("errors.player.playback"));
      }
    },

    seek: async (sec) => {
      try {
        await api().playbackSeek(sec);
      } catch (error) {
        notifyError(error, i18n.t("errors.player.seek"));
      }
    },

    // Optimistic: the store always holds the last requested mode, so quick
    // toggles compare against what was asked for, not an older answer.
    setMode: async (mode) => {
      const previous = get().mode;
      const seq = ++modeSeq;
      set({ mode });
      try {
        await api().playbackSetMode(mode);
      } catch (error) {
        if (seq === modeSeq) set({ mode: previous });
        notifyError(error, i18n.t("errors.player.hearing"));
      }
    },

    setFilter: async (value) => {
      set({ filter: Math.max(-1, Math.min(1, value)) });
      if (filterSending) return;
      filterSending = true;
      // Sends the newest value, then again if the knob moved meanwhile.
      const sendNewest = async (): Promise<void> => {
        const sending = get().filter;
        if (sending === filterAccepted) return;
        await api().playbackSetFilter(sending);
        filterAccepted = sending;
        await sendNewest();
      };
      try {
        await sendNewest();
      } catch (error) {
        set({ filter: filterAccepted });
        notifyError(error, i18n.t("errors.player.filter"));
      } finally {
        filterSending = false;
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
        notifyError(error, i18n.t("errors.player.cue"));
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
        notifyError(error, i18n.t("errors.player.hotCueSet", { slot }));
      }
    },

    clearHotCue: async (slot) => {
      const detail = get().detail;
      if (!detail) return;
      try {
        await api().hotcueClear(detail.track.id, slot);
        await refreshDetail();
      } catch (error) {
        notifyError(error, i18n.t("errors.player.hotCueClear", { slot }));
      }
    },

    updateHotCue: async (slot, name, color) => {
      const detail = get().detail;
      if (!detail) return;
      try {
        await api().hotcueUpdate(detail.track.id, slot, name, color);
        await refreshDetail();
      } catch (error) {
        notifyError(error, i18n.t("errors.player.hotCueChange", { slot }));
      }
    },

    onPlaybackState: ({ playing, ended }) => {
      set({ playing });
      if (!ended) return;
      const id = get().detail?.track.id;
      const next = id === undefined ? null : useSetList.getState().nextAfter(id);
      if (next === null) return;
      void (async () => {
        await get().load(next);
        // Another track picked meanwhile keeps the deck.
        if (get().detail?.track.id === next && !get().playing) await get().togglePlay();
      })();
    },
  };
});

import { create } from "zustand";

import { api } from "@/lib/ipc";
import { notifyError } from "@/lib/notify";
import type { PlaybackMode, TrackDetail, TrackId } from "@/lib/types";

/** Only the newest load may update the deck or the engine (clicks can overlap). */
let loadSeq = 0;

/**
 * Deck state that changes at human speed. The playhead position and meters
 * arrive at about 30 Hz and live in refs (hooks/usePlaybackRefs.ts), not here.
 */
interface PlayerState {
  detail: TrackDetail | null;
  playing: boolean;
  mode: PlaybackMode;
  load: (id: TrackId) => Promise<void>;
  togglePlay: () => Promise<void>;
  seek: (sec: number) => Promise<void>;
  setMode: (mode: PlaybackMode) => Promise<void>;
  /** Rust reports a playback state change (e.g. the track reached its end). */
  onPlaybackState: (playing: boolean) => void;
}

export const usePlayer = create<PlayerState>()((set, get) => ({
  detail: null,
  playing: false,
  mode: "original",

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

  onPlaybackState: (playing) => set({ playing }),

  setMode: async (mode) => {
    try {
      await api().playbackSetMode(mode);
      set({ mode });
    } catch (error) {
      notifyError(error, "Could not switch what you hear");
    }
  },
}));

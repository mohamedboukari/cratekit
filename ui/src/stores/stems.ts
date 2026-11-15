import { create } from "zustand";

import { api } from "@/lib/ipc";
import { notifyError } from "@/lib/notify";
import type { StemName, StemsInfo, TrackId } from "@/lib/types";

/** Name of the stems model Rust downloads on first use. */
export const STEMS_MODEL = "htdemucs";

/** Four stems, or two (vocals and instrumental = drums + bass + other). */
export type StemLayout = "four" | "two";

type PerStem<T> = Record<StemName, T>;

export const ALL_STEMS: readonly StemName[] = ["vocals", "drums", "bass", "other"];

function perStem<T>(value: T): PerStem<T> {
  return { vocals: value, drums: value, bass: value, other: value };
}

/** Only the newest load may write (the user can switch tracks quickly). */
let loadSeq = 0;

interface StemsState {
  trackId: TrackId | null;
  info: StemsInfo | null;
  status: "idle" | "loading" | "ready";
  /** Null until Rust answered; false means the model must be downloaded first. */
  modelReady: boolean | null;
  modelSizeBytes: number | null;
  layout: StemLayout;
  gains: PerStem<number>;
  muted: PerStem<boolean>;
  soloed: PerStem<boolean>;
  /** A new track is on the deck: Rust starts it with a neutral stem mix, so does the UI. */
  trackChanged: (id: TrackId | null) => void;
  /** Loads stems info and the model status for the loaded track. */
  load: (id: TrackId) => Promise<void>;
  split: (id: TrackId) => Promise<void>;
  downloadModel: () => Promise<void>;
  setLayout: (layout: StemLayout) => void;
  /** Applies to every stem in the list (two-stem layout drives three at once). */
  setGain: (stems: readonly StemName[], gain: number) => Promise<void>;
  toggleMute: (stems: readonly StemName[]) => Promise<void>;
  toggleSolo: (stems: readonly StemName[]) => Promise<void>;
  exportStems: (id: TrackId) => Promise<string[] | null>;
}

export const useStems = create<StemsState>()((set, get) => ({
  trackId: null,
  info: null,
  status: "idle",
  modelReady: null,
  modelSizeBytes: null,
  layout: "four",
  gains: perStem(1),
  muted: perStem(false),
  soloed: perStem(false),

  trackChanged: (id) => {
    loadSeq += 1;
    set({
      trackId: id,
      info: null,
      status: "idle",
      gains: perStem(1),
      muted: perStem(false),
      soloed: perStem(false),
    });
  },

  load: async (id) => {
    if (get().trackId !== id) get().trackChanged(id);
    const seq = ++loadSeq;
    set({ status: "loading" });
    const [info, models] = await Promise.allSettled([
      api().stemsInfo(id),
      api().modelsStatus(),
    ]);
    if (seq !== loadSeq) return;
    const model =
      models.status === "fulfilled"
        ? models.value.find((m) => m.name === STEMS_MODEL)
        : undefined;
    set({
      info: info.status === "fulfilled" ? info.value : null,
      status: "ready",
      modelReady: model ? model.downloaded : null,
      modelSizeBytes: model?.sizeBytes ?? null,
    });
    if (info.status === "rejected") notifyError(info.reason, "Could not read the stems");
  },

  split: async (id) => {
    try {
      await api().stemsSplit(id);
    } catch (error) {
      notifyError(error, "Could not split stems");
    }
  },

  downloadModel: async () => {
    try {
      await api().modelsDownload(STEMS_MODEL);
    } catch (error) {
      notifyError(error, "Could not download the stems model");
    }
  },

  setLayout: (layout) => set({ layout }),

  setGain: async (stems, gain) => {
    const previous = get().gains;
    const gains = { ...previous };
    for (const stem of stems) gains[stem] = gain;
    set({ gains });
    try {
      await Promise.all(stems.map((stem) => api().stemsSetGain(stem, gain)));
    } catch (error) {
      set({ gains: previous });
      notifyError(error, "Could not change the stem volume");
    }
  },

  toggleMute: async (stems) => {
    const previous = get().muted;
    // A group is muted only when all of its stems are; toggling sets them all the same way.
    const next = !stems.every((stem) => previous[stem]);
    const muted = { ...previous };
    for (const stem of stems) muted[stem] = next;
    set({ muted });
    try {
      await Promise.all(stems.map((stem) => api().stemsSetMute(stem, next)));
    } catch (error) {
      set({ muted: previous });
      notifyError(error, "Could not mute the stem");
    }
  },

  toggleSolo: async (stems) => {
    const previous = get().soloed;
    const next = !stems.every((stem) => previous[stem]);
    const soloed = { ...previous };
    for (const stem of stems) soloed[stem] = next;
    set({ soloed });
    try {
      await Promise.all(stems.map((stem) => api().stemsSetSolo(stem, next)));
    } catch (error) {
      set({ soloed: previous });
      notifyError(error, "Could not solo the stem");
    }
  },

  exportStems: async (id) => {
    try {
      return await api().stemsExport(id);
    } catch (error) {
      notifyError(error, "Could not export the stems");
      return null;
    }
  },
}));

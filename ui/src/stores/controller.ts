import { create } from "zustand";

import { i18n } from "@/i18n";
import { api } from "@/lib/ipc";
import { notifyError } from "@/lib/notify";
import type { ControlTarget, MidiLearned, MidiMapping } from "@/lib/types";

/** Only the newest mappings request may write (devices can change quickly). */
let loadSeq = 0;

/** MIDI mapping for the connected controller, and the MIDI learn flow (#40). */
interface ControllerState {
  deviceId: string | null;
  mappings: MidiMapping[];
  /** The target waiting for the user to move a control. */
  learning: ControlTarget | null;
  /** The last learned mapping, to confirm it (and say which target lost it). */
  lastLearned: MidiLearned | null;
  load: (deviceId: string) => Promise<void>;
  startLearn: (target: ControlTarget) => Promise<void>;
  cancelLearn: () => Promise<void>;
  /** Rust saved a learned mapping (and moved it away from `replaced`). */
  onLearned: (learned: MidiLearned) => void;
}

export const useController = create<ControllerState>()((set, get) => ({
  deviceId: null,
  mappings: [],
  learning: null,
  lastLearned: null,

  load: async (deviceId) => {
    const seq = ++loadSeq;
    set({ deviceId });
    try {
      const mappings = await api().midiMappings(deviceId);
      if (seq === loadSeq) set({ mappings });
    } catch (error) {
      if (seq === loadSeq) notifyError(error, i18n.t("errors.controller.load"));
    }
  },

  startLearn: async (target) => {
    set({ learning: target, lastLearned: null });
    try {
      await api().midiLearnStart(target);
    } catch (error) {
      set({ learning: null });
      notifyError(error, i18n.t("errors.controller.learnStart"));
    }
  },

  cancelLearn: async () => {
    if (get().learning === null) return;
    set({ learning: null });
    try {
      await api().midiLearnCancel();
    } catch (error) {
      notifyError(error, i18n.t("errors.controller.learnCancel"));
    }
  },

  onLearned: (learned) =>
    set((s) => ({
      learning: s.learning === learned.target ? null : s.learning,
      lastLearned: learned,
      mappings: s.mappings.map((m) => {
        if (m.target === learned.target) return { ...m, binding: learned.binding };
        if (m.target === learned.replaced) return { ...m, binding: null };
        return m;
      }),
    })),
}));

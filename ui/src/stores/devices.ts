import { create } from "zustand";

import { api } from "@/lib/ipc";
import type { AudioDevice, MidiDevice, StemsBackendStatus } from "@/lib/types";

/** Connected hardware and the stems backend, for the sidebar and status bar. */
interface DevicesState {
  midi: MidiDevice[];
  audio: AudioDevice[];
  backend: StemsBackendStatus | null;
  load: () => Promise<void>;
  /** Asks Rust again which backend splits stems (it follows the CPU only setting). */
  loadBackend: () => Promise<void>;
  setMidi: (devices: MidiDevice[]) => void;
}

/** An older backend answer must not overwrite a newer one. */
let backendSeq = 0;

export const useDevices = create<DevicesState>()((set, get) => ({
  midi: [],
  audio: [],
  backend: null,

  load: async () => {
    // Each part is optional: a failed command leaves that part empty instead
    // of failing the others.
    const [midi, audio] = await Promise.allSettled([
      api().midiListDevices(),
      api().audioListDevices(),
      get().loadBackend(),
    ]);
    set({
      midi: midi.status === "fulfilled" ? midi.value : [],
      audio: audio.status === "fulfilled" ? audio.value : [],
    });
  },

  loadBackend: async () => {
    const seq = ++backendSeq;
    try {
      const backend = await api().stemsBackendStatus();
      if (seq === backendSeq) set({ backend });
    } catch {
      // Keep the last answer: the backend does not change when asking fails.
    }
  },

  setMidi: (midi) => set({ midi }),
}));

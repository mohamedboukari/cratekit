import { create } from "zustand";

import { api } from "@/lib/ipc";
import type { AudioDevice, MidiDevice, StemsBackendStatus } from "@/lib/types";

/** Connected hardware and the stems backend, for the sidebar and status bar. */
interface DevicesState {
  midi: MidiDevice[];
  audio: AudioDevice[];
  backend: StemsBackendStatus | null;
  load: () => Promise<void>;
  setMidi: (devices: MidiDevice[]) => void;
}

export const useDevices = create<DevicesState>()((set) => ({
  midi: [],
  audio: [],
  backend: null,

  load: async () => {
    // Each part is optional: a missing command (not implemented in Rust yet)
    // leaves that part empty instead of failing the others.
    const [midi, audio, backend] = await Promise.allSettled([
      api().midiListDevices(),
      api().audioListDevices(),
      api().stemsBackendStatus(),
    ]);
    set({
      midi: midi.status === "fulfilled" ? midi.value : [],
      audio: audio.status === "fulfilled" ? audio.value : [],
      backend: backend.status === "fulfilled" ? backend.value : null,
    });
  },

  setMidi: (midi) => set({ midi }),
}));

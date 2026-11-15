import { create } from "zustand";

import { api } from "@/lib/ipc";
import { DEFAULT_SETTINGS } from "@/lib/defaults";
import { notifyError } from "@/lib/notify";
import type { Settings } from "@/lib/types";

interface SettingsState {
  settings: Settings | null;
  /** "loading" until the first answer; "ready" also when defaults were used. */
  status: "loading" | "ready";
  load: () => Promise<void>;
  /** Optimistic: updates the UI at once, reverts if Rust rejects the change. */
  set: <K extends keyof Settings>(key: K, value: Settings[K]) => Promise<void>;
  /** Saves one resizable panel group's layout (panel id to percentage). */
  setPaneLayout: (groupId: string, layout: Record<string, number>) => Promise<void>;
}

export const useSettings = create<SettingsState>()((set, get) => ({
  settings: null,
  status: "loading",

  load: async () => {
    try {
      set({ settings: await api().settingsGet(), status: "ready" });
    } catch (error) {
      // Never block the window on settings: fall back to defaults and say so.
      set({ settings: structuredClone(DEFAULT_SETTINGS), status: "ready" });
      notifyError(error, "Could not load settings, using defaults");
    }
  },

  set: async (key, value) => {
    const previous = get().settings;
    if (!previous) return;
    set({ settings: { ...previous, [key]: value } });
    try {
      await api().settingsSet(key, value);
    } catch (error) {
      set({ settings: previous });
      notifyError(error, "Could not save the setting");
    }
  },

  setPaneLayout: async (groupId, layout) => {
    const current = get().settings;
    if (!current) return;
    await get().set("paneLayouts", { ...current.paneLayouts, [groupId]: layout });
  },
}));

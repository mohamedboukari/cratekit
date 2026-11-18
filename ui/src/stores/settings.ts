import { create } from "zustand";

import { applyLanguage, i18n } from "@/i18n";
import { api } from "@/lib/ipc";
import { DEFAULT_SETTINGS } from "@/lib/defaults";
import { notifyError } from "@/lib/notify";
import type { Settings, UiLanguage } from "@/lib/types";

interface SettingsState {
  settings: Settings | null;
  /** "loading" until the first answer; "ready" also when defaults were used. */
  status: "loading" | "ready";
  load: () => Promise<void>;
  /** Optimistic: updates the UI at once, reverts if Rust rejects the change. */
  set: <K extends keyof Settings>(key: K, value: Settings[K]) => Promise<void>;
  /** Settings Rust broadcast after a change in any window (newer than any load in flight). */
  applyRemote: (settings: Settings) => void;
  /** Saves one resizable panel group's layout (panel id to percentage). */
  setPaneLayout: (groupId: string, layout: Record<string, number>) => Promise<void>;
}

/** The language last applied, so unrelated settings changes do not re-apply it. */
let appliedLanguage: UiLanguage | null = null;

/** Applies the settings' UI language when it changed (both windows call this). */
function followLanguage(settings: Settings): void {
  // Older saved settings have no language yet: English, the default.
  const language = settings.language ?? DEFAULT_SETTINGS.language;
  if (language === appliedLanguage) return;
  appliedLanguage = language;
  applyLanguage(language);
}

/** A load that started before a broadcast must not overwrite it. */
let loadSeq = 0;

export const useSettings = create<SettingsState>()((set, get) => ({
  settings: null,
  status: "loading",

  load: async () => {
    const seq = ++loadSeq;
    try {
      const settings = await api().settingsGet();
      if (seq === loadSeq) {
        set({ settings, status: "ready" });
        followLanguage(settings);
      }
    } catch (error) {
      if (seq !== loadSeq) return;
      // Never block the window on settings: fall back to defaults and say so.
      set({ settings: structuredClone(DEFAULT_SETTINGS), status: "ready" });
      notifyError(error, i18n.t("errors.settings.load"));
    }
  },

  set: async (key, value) => {
    const previous = get().settings;
    if (!previous) return;
    const next = { ...previous, [key]: value };
    set({ settings: next });
    followLanguage(next);
    try {
      await api().settingsSet(key, value);
    } catch (error) {
      // Revert only this key: other keys may have changed since (another window).
      const current = get().settings;
      if (current) {
        const reverted = { ...current, [key]: previous[key] };
        set({ settings: reverted });
        followLanguage(reverted);
      }
      notifyError(error, i18n.t("errors.settings.save"));
    }
  },

  applyRemote: (settings) => {
    loadSeq += 1;
    set({ settings, status: "ready" });
    followLanguage(settings);
  },

  setPaneLayout: async (groupId, layout) => {
    const current = get().settings;
    if (!current) return;
    await get().set("paneLayouts", { ...current.paneLayouts, [groupId]: layout });
  },
}));

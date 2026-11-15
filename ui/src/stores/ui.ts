import { create } from "zustand";

import { api, isMockMode } from "@/lib/ipc";
import { notifyError } from "@/lib/notify";
import type { TrackId } from "@/lib/types";

export type ToolTab = "analyze" | "master" | "stems" | "controller";

/** Pads show hot cues 1 to 4 (page 1) or 5 to 8 (page 2). */
export type PadPage = 1 | 2;

interface UiState {
  activeTool: ToolTab;
  padPage: PadPage;
  /** The in-page Settings window (browser demo only; the app opens a native window). */
  settingsOpen: boolean;
  /** Tracks waiting in the VirtualDJ export dialog, or null when it is closed. */
  exportIds: TrackId[] | null;
  setActiveTool: (tab: ToolTab) => void;
  togglePadPage: () => void;
  openSettings: () => void;
  closeSettings: () => void;
  openExport: (ids: TrackId[]) => void;
  closeExport: () => void;
}

export const useUi = create<UiState>()((set) => ({
  activeTool: "analyze",
  padPage: 1,
  settingsOpen: false,
  exportIds: null,
  setActiveTool: (activeTool) => set({ activeTool }),
  togglePadPage: () => set((s) => ({ padPage: s.padPage === 1 ? 2 : 1 })),
  // A browser has no second native window, so the demo shows Settings in the page.
  openSettings: () => {
    if (isMockMode()) set({ settingsOpen: true });
    else
      api()
        .appOpenSettings()
        .catch((error: unknown) => notifyError(error, "Could not open Settings"));
  },
  closeSettings: () => set({ settingsOpen: false }),
  openExport: (ids) => set({ exportIds: ids }),
  closeExport: () => set({ exportIds: null }),
}));

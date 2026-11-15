import { create } from "zustand";

export type ToolTab = "analyze" | "master" | "stems" | "controller";

/** Pads show hot cues 1 to 4 (page 1) or 5 to 8 (page 2). */
export type PadPage = 1 | 2;

interface UiState {
  activeTool: ToolTab;
  padPage: PadPage;
  setActiveTool: (tab: ToolTab) => void;
  togglePadPage: () => void;
}

export const useUi = create<UiState>()((set) => ({
  activeTool: "analyze",
  padPage: 1,
  setActiveTool: (activeTool) => set({ activeTool }),
  togglePadPage: () => set((s) => ({ padPage: s.padPage === 1 ? 2 : 1 })),
}));

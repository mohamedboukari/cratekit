import type { Settings } from "@/lib/types";

/**
 * Settings used until Rust answers, or when it cannot (for example while a
 * command is not implemented yet). Keeps the window usable with default pane
 * sizes instead of blocking on a failed load.
 */
export const DEFAULT_SETTINGS: Settings = {
  audioDeviceId: null,
  bufferSize: 512,
  watchedFolders: [],
  watchFolders: false,
  outputFolder: "",
  stemsBackend: "auto",
  paneLayouts: {},
};

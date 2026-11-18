import { useLibrary } from "@/stores/library";
import { usePlayer } from "@/stores/player";
import { useSetList } from "@/stores/setlist";

/**
 * Reloads what shows analysis results (the browser rows, the set list and the
 * loaded track) after Rust stored some: a finished, failed or cancelled job.
 */
export function refreshAfterAnalysis(): void {
  void useLibrary.getState().refreshRows();
  void useSetList.getState().refresh();
  void usePlayer.getState().refresh();
}

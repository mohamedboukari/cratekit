import { useEffect } from "react";

import type { PlaybackMode, TrackId } from "@/lib/types";
import { useMaster } from "@/stores/master";
import { usePlayer } from "@/stores/player";
import { useUi } from "@/stores/ui";

/** Only the newest tab or side change may switch the mode (the B side waits for a render). */
let hearingSeq = 0;

async function applyMode(trackId: TrackId, mode: PlaybackMode): Promise<void> {
  const seq = ++hearingSeq;
  // The B side needs a render with the current settings before it plays.
  if (mode === "mastered") await useMaster.getState().preview(trackId);
  if (seq !== hearingSeq) return;
  await usePlayer.getState().setMode(mode);
}

/**
 * What you hear follows the open tool tab (docs/UI.md, Interaction rules):
 * Analyze and Controller play the original, Master plays the A/B side,
 * Stems plays the stem mix once the track has stems. Mount once in the app root.
 */
export function useHearingFollowsTab(): void {
  const tab = useUi((s) => s.activeTool);
  const side = useMaster((s) => s.side);
  const trackId = usePlayer((s) => s.detail?.track.id ?? null);
  const hasStems = usePlayer((s) => s.detail?.track.hasStems ?? false);

  useEffect(() => {
    if (trackId === null) return;
    let mode: PlaybackMode = "original";
    if (tab === "master" && side === "b") mode = "mastered";
    else if (tab === "stems" && hasStems) mode = "stems";
    // Always sent (cheap): Rust may have reset the mode when the track loaded.
    void applyMode(trackId, mode);
  }, [tab, side, trackId, hasStems]);
}

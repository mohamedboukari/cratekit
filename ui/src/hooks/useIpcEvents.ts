import { useEffect } from "react";
import { toast } from "sonner";

import { api, type Unlisten } from "@/lib/ipc";
import { useJobs } from "@/stores/jobs";
import { useLibrary } from "@/stores/library";
import { usePlayer } from "@/stores/player";

/**
 * Wires Rust events that change human-speed state into the stores. Mount once
 * in the app root. High-frequency events (position, meters) are handled by
 * hooks/usePlaybackRefs.ts instead, so they never touch React state.
 */
export function useIpcEvents(): void {
  useEffect(() => {
    const subscriptions: Array<Promise<Unlisten>> = [
      api().onJobProgress((p) => useJobs.getState().onProgress(p)),
      api().onJobDone((d) => {
        useJobs.getState().onDone(d);
        if (d.kind === "analysis" || d.kind === "stems")
          void useLibrary.getState().refreshRows();
      }),
      api().onJobFailed((f) => useJobs.getState().onFailed(f)),
      api().onPlaybackState((s) => usePlayer.getState().onPlaybackState(s.playing)),
      api().onAudioDeviceChanged((change) => {
        if (change.reason === "unplugged") {
          toast.warning("Audio device disconnected", {
            description: "Playback switched to the system default output.",
          });
        }
      }),
    ];
    return () => {
      for (const sub of subscriptions) void sub.then((stop) => stop());
    };
  }, []);
}

import { useEffect } from "react";
import { toast } from "sonner";

import { i18n } from "@/i18n";
import { api, type Unlisten } from "@/lib/ipc";
import type { ControlTarget, StemName } from "@/lib/types";
import { useController } from "@/stores/controller";
import { useDevices } from "@/stores/devices";
import { useJobs } from "@/stores/jobs";
import { useMaster } from "@/stores/master";
import { refreshAfterAnalysis } from "@/stores/refresh";
import { usePlayer } from "@/stores/player";
import { useSettings } from "@/stores/settings";
import { useStems } from "@/stores/stems";
import { useUi } from "@/stores/ui";

/** Controller pads that mute a stem. */
const STEM_MUTES: Partial<Record<ControlTarget, StemName>> = {
  "mute-vocals": "vocals",
  "mute-drums": "drums",
  "mute-bass": "bass",
  "mute-other": "other",
};

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
        if (d.kind === "analysis" || d.kind === "stems") refreshAfterAnalysis();
        if (d.kind === "model-download")
          toast.success(i18n.t("common.events.modelReady"));
        // New stems or a freshly downloaded model change what the Stems tab can show.
        const stemsTrack = useStems.getState().trackId;
        if ((d.kind === "stems" || d.kind === "model-download") && stemsTrack !== null)
          void useStems.getState().load(stemsTrack);
      }),
      api().onJobFailed((f) => {
        useJobs.getState().onFailed(f);
        // A failed analysis still stored the tracks that worked.
        if (f.kind === "analysis") refreshAfterAnalysis();
      }),
      api().onPlaybackState((s) => usePlayer.getState().onPlaybackState(s.playing)),
      api().onMidiDeviceChanged((devices) => useDevices.getState().setMidi(devices)),
      api().onMidiLearned((learned) => useController.getState().onLearned(learned)),
      // The Settings window is a separate page; Rust tells this one what changed.
      api().onSettingsChanged((settings) => useSettings.getState().applyRemote(settings)),
      // Rust runs engine actions itself; these targets change UI-owned state, so the UI runs them.
      api().onControlEvent((event) => {
        if (event.value <= 0) return;
        const stem = STEM_MUTES[event.target];
        if (stem) void useStems.getState().toggleMute([stem]);
        else if (event.target === "ab-toggle" && useUi.getState().activeTool === "master")
          useMaster.getState().toggleSide();
        else if (event.target === "pad-page") useUi.getState().togglePadPage();
      }),
      api().onAudioDeviceChanged((change) => {
        if (change.reason === "unplugged") {
          toast.warning(i18n.t("common.events.deviceUnplugged"), {
            description: i18n.t("common.events.deviceUnpluggedDetail"),
          });
        }
      }),
    ];
    return () => {
      for (const sub of subscriptions) void sub.then((stop) => stop());
    };
  }, []);
}

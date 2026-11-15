import { useEffect, type JSX } from "react";

import { BrowserArea } from "@/components/browser/BrowserArea";
import { DropOverlay } from "@/components/dialogs/DropOverlay";
import { ExportDialog } from "@/components/dialogs/ExportDialog";
import { Deck } from "@/components/deck/Deck";
import { AppShell } from "@/components/layout/AppShell";
import { DemoBanner } from "@/components/layout/DemoBanner";
import { Sidebar } from "@/components/layout/Sidebar";
import { SettingsDialog } from "@/components/settings/SettingsDialog";
import { StatusBar } from "@/components/layout/StatusBar";
import { ToolPanel } from "@/components/tools/ToolPanel";
import { Toaster } from "@/components/ui/sonner";
import { useHearingFollowsTab } from "@/hooks/useHearing";
import { useIpcEvents } from "@/hooks/useIpcEvents";
import { usePlaybackPositionRef } from "@/hooks/usePlaybackRefs";
import { useShortcuts } from "@/hooks/useShortcuts";
import { isMockMode } from "@/lib/ipc";
import type { CueSlot } from "@/lib/types";
import { useDevices } from "@/stores/devices";
import { useLibrary } from "@/stores/library";
import { useMaster } from "@/stores/master";
import { usePlayer } from "@/stores/player";
import { useSetList } from "@/stores/setlist";
import { useSettings } from "@/stores/settings";
import { useStems } from "@/stores/stems";
import { useUi } from "@/stores/ui";

/** Build-time flag: false in production Tauri builds, so the banner is not even bundled. */
const SHOW_DEMO_BANNER = import.meta.env.DEV || import.meta.env.VITE_MOCK === "1";
/** Same flag: Settings opens as an in-page window only in the browser demo. */
const IN_PAGE_SETTINGS = SHOW_DEMO_BANNER;

/** Main window. Settings load first so saved pane sizes apply on the first paint. */
export function App(): JSX.Element {
  const ready = useSettings((s) => s.status === "ready");
  const loadSettings = useSettings((s) => s.load);
  const loadLibrary = useLibrary((s) => s.load);
  const togglePlay = usePlayer((s) => s.togglePlay);
  const cue = usePlayer((s) => s.cue);
  const triggerHotCue = usePlayer((s) => s.triggerHotCue);
  const loadDevices = useDevices((s) => s.load);
  const loadSetList = useSetList((s) => s.load);
  const loadPresets = useMaster((s) => s.loadPresets);
  const toggleSide = useMaster((s) => s.toggleSide);
  const toggleMute = useStems((s) => s.toggleMute);
  const loadCount = usePlayer((s) => s.loadCount);
  const position = usePlaybackPositionRef();

  useIpcEvents();
  useHearingFollowsTab();
  const hotCue = (slot: CueSlot) => () => void triggerHotCue(slot, position.current);
  useShortcuts({
    "play-pause": () => void togglePlay(),
    cue: () => void cue(),
    "hotcue-1": hotCue(1),
    "hotcue-2": hotCue(2),
    "hotcue-3": hotCue(3),
    "hotcue-4": hotCue(4),
    "hotcue-5": hotCue(5),
    "hotcue-6": hotCue(6),
    "hotcue-7": hotCue(7),
    "hotcue-8": hotCue(8),
    "mute-vocals": () => void toggleMute(["vocals"]),
    "mute-drums": () => void toggleMute(["drums"]),
    "mute-bass": () => void toggleMute(["bass"]),
    "mute-other": () => void toggleMute(["other"]),
    // A/B lives in the Master tab (SPEC, Module 5); elsewhere B would flip a side you cannot hear.
    "open-settings": () => useUi.getState().openSettings(),
    "ab-toggle": () => {
      if (useUi.getState().activeTool === "master") toggleSide();
    },
  });

  // Every load starts with a neutral stem mix, in Rust and here.
  useEffect(() => {
    if (loadCount > 0)
      useStems.getState().trackChanged(usePlayer.getState().detail?.track.id ?? null);
  }, [loadCount]);

  useEffect(() => {
    void loadSettings().then(() => loadSetList());
    void loadLibrary();
    void loadDevices();
    void loadPresets();
  }, [loadSettings, loadLibrary, loadDevices, loadSetList, loadPresets]);

  return (
    <>
      {ready ? (
        <AppShell
          banner={SHOW_DEMO_BANNER && isMockMode() ? <DemoBanner /> : undefined}
          sidebar={<Sidebar />}
          deck={<Deck />}
          browser={<BrowserArea />}
          tools={<ToolPanel />}
          statusBar={<StatusBar />}
        />
      ) : (
        <div
          className="grid h-full place-items-center text-muted-foreground"
          aria-busy="true"
        >
          Loading Cratekit...
        </div>
      )}
      <DropOverlay />
      <ExportDialog />
      {IN_PAGE_SETTINGS && isMockMode() && <SettingsDialog />}
      <Toaster position="bottom-right" />
    </>
  );
}

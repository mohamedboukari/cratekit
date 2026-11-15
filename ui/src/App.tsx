import { useEffect, type JSX } from "react";

import { BrowserArea } from "@/components/browser/BrowserArea";
import { Deck } from "@/components/deck/Deck";
import { AppShell } from "@/components/layout/AppShell";
import { DemoBanner } from "@/components/layout/DemoBanner";
import { Sidebar } from "@/components/layout/Sidebar";
import { StatusBar } from "@/components/layout/StatusBar";
import { ToolPanel } from "@/components/tools/ToolPanel";
import { Toaster } from "@/components/ui/sonner";
import { useIpcEvents } from "@/hooks/useIpcEvents";
import { usePlaybackPositionRef } from "@/hooks/usePlaybackRefs";
import { useShortcuts } from "@/hooks/useShortcuts";
import { isMockMode } from "@/lib/ipc";
import type { CueSlot } from "@/lib/types";
import { useDevices } from "@/stores/devices";
import { useLibrary } from "@/stores/library";
import { usePlayer } from "@/stores/player";
import { useSetList } from "@/stores/setlist";
import { useSettings } from "@/stores/settings";

/** Build-time flag: false in production Tauri builds, so the banner is not even bundled. */
const SHOW_DEMO_BANNER = import.meta.env.DEV || import.meta.env.VITE_MOCK === "1";

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
  const position = usePlaybackPositionRef();

  useIpcEvents();
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
  });

  useEffect(() => {
    void loadSettings().then(() => loadSetList());
    void loadLibrary();
    void loadDevices();
  }, [loadSettings, loadLibrary, loadDevices, loadSetList]);

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
      <Toaster position="bottom-right" />
    </>
  );
}

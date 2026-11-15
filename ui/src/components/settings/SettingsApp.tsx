import { useEffect, type JSX } from "react";

import { SettingsView } from "@/components/settings/SettingsView";
import { Toaster } from "@/components/ui/sonner";
import { api } from "@/lib/ipc";
import { useDevices } from "@/stores/devices";
import { useSettings } from "@/stores/settings";

/**
 * Root of the native Settings window (opened from the app menu or Cmd/Ctrl+,).
 * It is a separate page, so it loads its own settings and devices and listens
 * for changes Rust broadcasts.
 */
export function SettingsApp(): JSX.Element {
  const loadSettings = useSettings((s) => s.load);
  const loadDevices = useDevices((s) => s.load);

  useEffect(() => {
    void loadSettings();
    void loadDevices();
    const subscriptions = [
      api().onSettingsChanged((settings) => useSettings.getState().applyRemote(settings)),
      api().onMidiDeviceChanged((devices) => useDevices.getState().setMidi(devices)),
      api().onAudioDeviceChanged(() => void useDevices.getState().load()),
    ];
    return () => {
      for (const sub of subscriptions) void sub.then((stop) => stop());
    };
  }, [loadSettings, loadDevices]);

  return (
    <main className="h-full bg-background text-foreground">
      <h1 className="sr-only">Cratekit settings</h1>
      <SettingsView />
      <Toaster />
    </main>
  );
}

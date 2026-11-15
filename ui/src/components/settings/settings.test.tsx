import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it } from "vitest";

import { CueEditDialog } from "@/components/deck/CueEditDialog";
import { ExportDialog } from "@/components/dialogs/ExportDialog";
import { SettingsDialog } from "@/components/settings/SettingsDialog";
import { SettingsView } from "@/components/settings/SettingsView";
import { DEFAULT_SETTINGS } from "@/lib/defaults";
import { setApiForTests } from "@/lib/ipc";
import { createMockApi } from "@/mock/api";
import { useDevices } from "@/stores/devices";
import { useLibrary } from "@/stores/library";
import { usePlayer } from "@/stores/player";
import { useSettings } from "@/stores/settings";
import { useUi } from "@/stores/ui";

describe("settings, dialogs and first run", () => {
  beforeEach(async () => {
    setApiForTests(createMockApi());
    useSettings.setState({
      settings: structuredClone(DEFAULT_SETTINGS),
      status: "ready",
    });
    await useDevices.getState().load();
    useUi.setState({ settingsOpen: false, exportIds: null });
  });

  it("adds a watched folder through the folder picker and switches to CPU only", async () => {
    render(<SettingsView />);
    fireEvent.click(screen.getByRole("button", { name: "Library" }));
    fireEvent.click(screen.getByRole("button", { name: "Add folder..." }));
    await waitFor(() =>
      expect(useSettings.getState().settings?.watchedFolders).toEqual([
        "~/Music/Incoming",
      ]),
    );
    fireEvent.click(screen.getByRole("button", { name: "Processing" }));
    fireEvent.click(screen.getByRole("radio", { name: /CPU only/ }));
    await waitFor(() =>
      expect(useSettings.getState().settings?.stemsBackend).toBe("cpu"),
    );
  });

  it("opens Settings in the page in the browser demo, and a native window in the app", async () => {
    render(<SettingsDialog />);
    act(() => useUi.getState().openSettings());
    expect(await screen.findByRole("dialog", { name: "Settings" })).toBeInTheDocument();

    const opened: string[] = [];
    const mock = createMockApi();
    setApiForTests(
      {
        ...mock,
        appOpenSettings: () => {
          opened.push("native");
          return mock.appOpenSettings();
        },
      },
      false,
    );
    act(() => useUi.setState({ settingsOpen: false }));
    useUi.getState().openSettings();
    await waitFor(() => expect(opened).toEqual(["native"]));
    expect(useUi.getState().settingsOpen).toBe(false);
  });

  it("reverts only the setting that failed to save", async () => {
    const mock = createMockApi();
    setApiForTests({
      ...mock,
      settingsSet: (key, value) =>
        key === "bufferSize"
          ? Promise.reject({ code: "io", message: "disk full" })
          : mock.settingsSet(key, value),
    });
    const pending = useSettings.getState().set("bufferSize", 1024);
    // Another window changed a different key meanwhile.
    useSettings.getState().applyRemote({
      ...structuredClone(DEFAULT_SETTINGS),
      bufferSize: 1024,
      stemsBackend: "cpu",
    });
    await pending;
    expect(useSettings.getState().settings).toMatchObject({
      bufferSize: DEFAULT_SETTINGS.bufferSize,
      stemsBackend: "cpu",
    });
  });

  it("shows the VirtualDJ dry run before writing anything", async () => {
    const calls: boolean[] = [];
    const mock = createMockApi();
    setApiForTests({
      ...mock,
      exportVirtualdj: (ids, dryRun) => {
        calls.push(dryRun);
        return mock.exportVirtualdj(ids, dryRun);
      },
    });
    render(<ExportDialog />);
    useUi.getState().openExport([1, 2]);
    const dialog = await screen.findByRole("dialog");
    expect(await within(dialog).findByText(/2 tracks/)).toBeInTheDocument();
    expect(within(dialog).getByText(/Backup:/)).toBeInTheDocument();
    expect(calls).toEqual([true]);
    fireEvent.click(within(dialog).getByRole("button", { name: "Export" }));
    await waitFor(() => expect(useUi.getState().exportIds).toBeNull());
    expect(calls).toEqual([true, false]);
  });

  it("imports picked files into the library", async () => {
    await useLibrary.getState().load();
    const before = useLibrary.getState().counts?.all ?? 0;
    const added = await useLibrary
      .getState()
      .addPaths(["~/Music/A.mp3", "~/Music/B.flac"]);
    expect(added).toBe(2);
    expect(useLibrary.getState().counts?.all).toBe(before + 2);
  });

  it("renames and recolours a hot cue", async () => {
    await usePlayer.getState().load(1);
    const cue = usePlayer.getState().detail?.cues[0] ?? null;
    expect(cue).not.toBeNull();
    render(<CueEditDialog cue={cue} onClose={() => undefined} />);
    const dialog = await screen.findByRole("dialog");
    fireEvent.change(within(dialog).getByRole("textbox", { name: "Name" }), {
      target: { value: "Big drop" },
    });
    fireEvent.click(within(dialog).getByRole("radio", { name: "teal" }));
    fireEvent.click(within(dialog).getByRole("button", { name: "Save" }));
    await waitFor(() =>
      expect(usePlayer.getState().detail?.cues[0]).toMatchObject({
        name: "Big drop",
        color: "teal",
      }),
    );
  });
});

describe("deck shortcuts and dialogs", () => {
  it("ignores deck keys while a dialog has focus", async () => {
    const { renderHook } = await import("@testing-library/react");
    const { useShortcuts } = await import("@/hooks/useShortcuts");
    const pressed: string[] = [];
    renderHook(() => useShortcuts({ cue: () => pressed.push("cue") }));
    const dialog = document.createElement("div");
    dialog.setAttribute("role", "dialog");
    const button = document.createElement("button");
    dialog.append(button);
    document.body.append(dialog);
    fireEvent.keyDown(button, { code: "KeyC", key: "c" });
    fireEvent.keyDown(document.body, { code: "KeyC", key: "c" });
    expect(pressed).toEqual(["cue"]);
    dialog.remove();
  });
});

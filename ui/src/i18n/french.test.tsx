import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it } from "vitest";

import { App } from "@/App";
import { SettingsView } from "@/components/settings/SettingsView";
import { DEFAULT_SETTINGS } from "@/lib/defaults";
import { audioFormatsText } from "@/lib/formats";
import { jobLabel } from "@/lib/format";
import { setApiForTests } from "@/lib/ipc";
import { createMockApi } from "@/mock/api";
import { useDevices } from "@/stores/devices";
import { useSettings } from "@/stores/settings";
import { usePlayer } from "@/stores/player";

/** i18next shows the key itself when a translation is missing, e.g. "deck.transport.play". */
const RAW_KEY =
  /\b(common|deck|layout|browser|dialogs|tools|settings|shortcuts|errors)\.[a-z][A-Za-z]*(\.[A-Za-z_]+)*\b/;

/** All the text a user or a screen reader gets from the page. */
function pageText(): string {
  const labels = [
    ...document.querySelectorAll("[aria-label], [title], [placeholder]"),
  ].flatMap((el) => [
    el.getAttribute("aria-label") ?? "",
    el.getAttribute("title") ?? "",
    el.getAttribute("placeholder") ?? "",
  ]);
  return [document.body.textContent ?? "", ...labels].join("\n");
}

describe("French", () => {
  beforeEach(() => {
    setApiForTests(createMockApi());
  });

  afterEach(() => {
    // The mock keeps settings in localStorage: start the next test in English,
    // through the settings store, exactly as the app switches language.
    globalThis.localStorage.clear();
    useSettings.getState().applyRemote(structuredClone(DEFAULT_SETTINGS));
  });

  it("renders the whole main window with no missing translation", async () => {
    // The language comes from the saved settings, as for a user.
    const mock = createMockApi();
    await mock.settingsSet("language", "fr");
    setApiForTests(mock);
    render(<App />);
    expect(await screen.findByText("Friday club set")).toBeInTheDocument();
    await usePlayer.getState().load(3);
    await waitFor(() => expect(document.body.textContent).toContain("Tonalité"));
    const text = pageText();
    expect(text).not.toMatch(RAW_KEY);
  });

  it("switches the visible text when French is picked in Settings, and back", async () => {
    useSettings.setState({
      settings: structuredClone(DEFAULT_SETTINGS),
      status: "ready",
    });
    await useDevices.getState().load();
    render(<SettingsView />);
    const language = screen.getByRole("combobox", { name: "Language" });
    fireEvent.change(language, { target: { value: "fr" } });
    expect(await screen.findByRole("combobox", { name: "Langue" })).toBeInTheDocument();
    expect(pageText()).not.toMatch(RAW_KEY);
    fireEvent.change(screen.getByRole("combobox", { name: "Langue" }), {
      target: { value: "en" },
    });
    expect(await screen.findByRole("combobox", { name: "Language" })).toBeInTheDocument();
  });

  it("names background jobs and lists formats in the UI language", async () => {
    const analysis = { kind: "analysis", trackIds: [1, 2, 3], stage: null } as const;
    const verify = { kind: "model-download", trackIds: [], stage: "verify" } as const;
    expect(jobLabel(analysis)).toBe("Analyzing 3 tracks");
    expect(jobLabel({ ...analysis, trackIds: [1] })).toBe("Analyzing 1 track");
    expect(audioFormatsText("en")).toBe("MP3, WAV, FLAC, AIFF, M4A/AAC, and OGG");
    // Switch the way the app does: through the settings store.
    useSettings
      .getState()
      .applyRemote({ ...structuredClone(DEFAULT_SETTINGS), language: "fr" });
    expect(jobLabel(analysis)).toBe("Analyse de 3 morceaux");
    expect(jobLabel(verify)).toBe("Vérification du modèle de stems");
    expect(audioFormatsText("fr")).toBe("MP3, WAV, FLAC, AIFF, M4A/AAC et OGG");
  });
});

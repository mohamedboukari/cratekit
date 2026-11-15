import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import type { JSX } from "react";
import { beforeEach, describe, expect, it } from "vitest";

import { ToolPanel } from "@/components/tools/ToolPanel";
import { useHearingFollowsTab } from "@/hooks/useHearing";
import { setApiForTests } from "@/lib/ipc";
import { createMockApi } from "@/mock/api";
import { DEFAULT_QUERY, useLibrary } from "@/stores/library";
import { useMaster } from "@/stores/master";
import { usePlayer } from "@/stores/player";
import { useUi } from "@/stores/ui";

function Harness(): JSX.Element {
  useHearingFollowsTab();
  return <ToolPanel />;
}

describe("tool panel", () => {
  beforeEach(async () => {
    setApiForTests(createMockApi());
    useUi.setState({ activeTool: "analyze" });
    useMaster.setState({ side: "b" });
    useLibrary.setState({ query: DEFAULT_QUERY, rows: [], selectedIds: [] });
    usePlayer.setState({ detail: null, playing: false, mode: "original" });
    await usePlayer.getState().load(1);
  });

  it("shows the analysis and filters the browser from a mixes-well-with chip", async () => {
    render(<Harness />);
    const analysis = usePlayer.getState().detail?.analysis;
    expect(screen.getByText(analysis?.keyName ?? "missing")).toBeInTheDocument();
    const code = analysis?.mixesWellWith[0] ?? "missing";
    fireEvent.click(
      screen.getByRole("button", { name: `Show tracks that match ${code}` }),
    );
    await waitFor(() => expect(useLibrary.getState().query.matchCamelot).toBe(code));
  });

  it("switches what you hear with the tab: Master plays the A/B side", async () => {
    render(<Harness />);
    act(() => useUi.getState().setActiveTool("master"));
    await waitFor(() => expect(usePlayer.getState().mode).toBe("mastered"));
    act(() => useMaster.getState().setSide("a"));
    await waitFor(() => expect(usePlayer.getState().mode).toBe("original"));
    act(() => useUi.getState().setActiveTool("controller"));
    await waitFor(() => expect(usePlayer.getState().mode).toBe("original"));
  });
});

import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import type { JSX } from "react";
import { toast } from "sonner";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { ToolPanel } from "@/components/tools/ToolPanel";
import { useHearingFollowsTab } from "@/hooks/useHearing";
import { setApiForTests } from "@/lib/ipc";
import { createMockApi } from "@/mock/api";
import { useJobs } from "@/stores/jobs";
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
    useJobs.setState({ jobs: {}, paused: false });
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

  it("exports as a job: progress and Cancel in the Master tab, then Export again", async () => {
    const error = vi.spyOn(toast, "error");
    const mock = createMockApi();
    setApiForTests(mock);
    await mock.onJobProgress((p) => useJobs.getState().onProgress(p));
    await mock.onJobDone((d) => useJobs.getState().onDone(d));
    useUi.setState({ activeTool: "master" });
    render(<Harness />);

    fireEvent.click(screen.getByRole("button", { name: "Export WAV" }));
    expect(
      await screen.findByRole("progressbar", { name: "Export progress" }),
    ).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Export WAV" })).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));

    expect(await screen.findByRole("button", { name: "Export WAV" })).toBeEnabled();
    expect(useMaster.getState().exporting).toBe(false);
    expect(error).not.toHaveBeenCalled();
    error.mockRestore();
  });

  it("names the exporting track when another one is on the deck", () => {
    useUi.setState({ activeTool: "master" });
    useMaster.setState({ exportTitle: "Other track" });
    render(<Harness />);
    act(() =>
      useJobs.getState().onProgress({
        jobId: "job-7",
        kind: "master",
        label: "Mastering a track",
        stage: null,
        trackIds: [2],
        done: 40,
        total: 100,
        queued: false,
      }),
    );
    expect(screen.getByText("Exporting Other track")).toBeInTheDocument();
    expect(screen.getByRole("progressbar", { name: "Export progress" })).toHaveAttribute(
      "value",
      "40",
    );
    expect(screen.getByText("40%")).toBeInTheDocument();
    useMaster.setState({ exportTitle: null });
  });
});

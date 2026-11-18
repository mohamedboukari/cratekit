import { act, render, renderHook, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { StemsPanel } from "@/components/tools/StemsPanel";
import { useHearingFollowsTab } from "@/hooks/useHearing";
import { useIpcEvents } from "@/hooks/useIpcEvents";
import { setApiForTests, type CratekitApi, type Listener } from "@/lib/ipc";
import type { ControlEvent, MasterSettings, TrackId } from "@/lib/types";
import { createMockApi } from "@/mock/api";
import { useJobs } from "@/stores/jobs";
import { useMaster } from "@/stores/master";
import { usePlayer } from "@/stores/player";
import { useStems } from "@/stores/stems";
import { useUi } from "@/stores/ui";

describe("tool tab regressions", () => {
  beforeEach(() => {
    setApiForTests(createMockApi());
    useUi.setState({ activeTool: "analyze" });
    useMaster.setState({ side: "b" });
    useJobs.setState({ jobs: {}, paused: false });
    useStems.getState().trackChanged(null);
    usePlayer.setState({ detail: null, playing: false, mode: "original" });
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it("keeps the loaded track's mixer when another track is splitting", async () => {
    await usePlayer.getState().load(3); // track 3 has stems in the fixtures
    useJobs.getState().onProgress({
      jobId: "other",
      kind: "stems",
      label: "Splitting stems",
      stage: null,
      trackIds: [5],
      done: 4,
      total: 20,
      queued: false,
    });
    render(<StemsPanel />);
    expect(await screen.findByRole("button", { name: "4 stems" })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Cancel" })).not.toBeInTheDocument();
  });

  it("ends on the side that was picked last when B is pressed twice quickly", async () => {
    await usePlayer.getState().load(1);
    useUi.setState({ activeTool: "master" });
    renderHook(() => useHearingFollowsTab());
    act(() => useMaster.getState().toggleSide());
    act(() => useMaster.getState().toggleSide());
    await waitFor(() => expect(usePlayer.getState().mode).toBe("mastered"));
  });

  it("renders the B side for the track on the deck, not the one that was there before", async () => {
    const previews: TrackId[] = [];
    const mock = createMockApi();
    const api: CratekitApi = {
      ...mock,
      masterPreview: (id: TrackId, settings: MasterSettings) => {
        previews.push(id);
        return mock.masterPreview(id, settings);
      },
    };
    setApiForTests(api);
    await usePlayer.getState().load(1);
    useUi.setState({ activeTool: "master" });
    vi.useFakeTimers();
    useMaster.getState().setIntensity(0.3);
    await usePlayer.getState().load(2);
    await vi.advanceTimersByTimeAsync(300);
    expect(previews).toEqual([2]);
  });

  it("starts every newly loaded track with a neutral stem mix", async () => {
    await useStems.getState().toggleMute(["vocals"]);
    useStems.getState().trackChanged(4);
    expect(useStems.getState().muted.vocals).toBe(false);
  });

  it("runs controller mute and A/B presses on the UI state", async () => {
    let control: Listener<ControlEvent> | null = null;
    const mock = createMockApi();
    setApiForTests({
      ...mock,
      onControlEvent: (listener) => {
        control = listener;
        return mock.onControlEvent(listener);
      },
    });
    renderHook(() => useIpcEvents());
    await waitFor(() => expect(control).not.toBeNull());
    const press = (target: ControlEvent["target"]): void =>
      control?.({ target, value: 1 });
    act(() => press("mute-vocals"));
    await waitFor(() => expect(useStems.getState().muted.vocals).toBe(true));
    act(() => press("ab-toggle"));
    expect(useMaster.getState().side).toBe("b"); // not on the Master tab: ignored
    useUi.setState({ activeTool: "master" });
    act(() => press("ab-toggle"));
    expect(useMaster.getState().side).toBe("a");
  });
});

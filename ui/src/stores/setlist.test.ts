import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { DEFAULT_SETTINGS } from "@/lib/defaults";
import { setApiForTests, type CratekitApi } from "@/lib/ipc";
import { createMockApi } from "@/mock/api";
import { usePlayer } from "@/stores/player";
import { useSetList } from "@/stores/setlist";
import { useSettings } from "@/stores/settings";

describe("set list store", () => {
  beforeEach(() => {
    setApiForTests(createMockApi());
    useSettings.setState({
      settings: structuredClone(DEFAULT_SETTINGS),
      status: "ready",
    });
    useSetList.setState({ items: [], transitions: [] });
  });

  it("adds tracks once, keeps order, and persists the ids in settings", async () => {
    await useSetList.getState().add([3, 5]);
    await useSetList.getState().add([5, 7]);
    expect(useSetList.getState().items.map((t) => t.id)).toEqual([3, 5, 7]);
    expect(useSettings.getState().settings?.setList).toEqual([3, 5, 7]);
  });

  it("inserts at an index, moves and removes", async () => {
    await useSetList.getState().add([1, 2, 3]);
    await useSetList.getState().add([9], 1);
    expect(useSetList.getState().items.map((t) => t.id)).toEqual([1, 9, 2, 3]);
    await useSetList.getState().move(3, 0);
    expect(useSetList.getState().items.map((t) => t.id)).toEqual([3, 1, 9, 2]);
    await useSetList.getState().remove(9);
    expect(useSetList.getState().items.map((t) => t.id)).toEqual([3, 1, 2]);
  });

  it("asks Rust for one transition per neighbour pair", async () => {
    await useSetList.getState().add([2, 8, 14]);
    const transitions = useSetList.getState().transitions;
    expect(transitions).toHaveLength(2);
    expect(transitions[0]).toMatchObject({ fromId: 2, toId: 8 });
    expect(typeof transitions[0]?.keyCompatible).toBe("boolean");
    expect(typeof transitions[0]?.bpmJump).toBe("boolean");
  });

  it("restores the saved list from settings", async () => {
    useSettings.setState({
      settings: { ...structuredClone(DEFAULT_SETTINGS), setList: [4, 6] },
      status: "ready",
    });
    await useSetList.getState().load();
    expect(useSetList.getState().items.map((t) => t.id)).toEqual([4, 6]);
  });

  it("drops tracks that are gone from the library and saves the shorter list", async () => {
    useSettings.setState({
      settings: { ...structuredClone(DEFAULT_SETTINGS), setList: [4, 9999, 6] },
      status: "ready",
    });
    await useSetList.getState().load();
    expect(useSetList.getState().items.map((t) => t.id)).toEqual([4, 6]);
    expect(useSettings.getState().settings?.setList).toEqual([4, 6]);
  });

  it("never overwrites the saved list when Rust fails to read it", async () => {
    const mock = createMockApi();
    const failing: CratekitApi = {
      ...mock,
      trackDetail: () => Promise.reject({ code: "io", message: "disk busy" }),
    };
    setApiForTests(failing);
    useSettings.setState({
      settings: { ...structuredClone(DEFAULT_SETTINGS), setList: [4, 6] },
      status: "ready",
    });
    await useSetList.getState().load();
    await useSetList.getState().add([8]);
    expect(useSetList.getState().items).toEqual([]);
    expect(useSettings.getState().settings?.setList).toEqual([4, 6]);
  });

  it("knows the track after another, and nothing after the last", async () => {
    await useSetList.getState().add([4, 2, 9]);
    const { nextAfter } = useSetList.getState();
    expect(nextAfter(4)).toBe(2);
    expect(nextAfter(2)).toBe(9);
    expect(nextAfter(9)).toBeNull();
    expect(nextAfter(1)).toBeNull();
  });
});

describe("playing through the set list", () => {
  let mock: CratekitApi;

  beforeEach(() => {
    mock = createMockApi();
    setApiForTests(mock);
    useSettings.setState({
      settings: structuredClone(DEFAULT_SETTINGS),
      status: "ready",
    });
    useSetList.setState({ items: [], transitions: [] });
    usePlayer.setState({ detail: null, playing: false, mode: "original" });
  });

  afterEach(async () => {
    // Stops the mock's playhead ticker.
    await mock.playbackPause();
  });

  const ended = { playing: false, ended: true };

  it("loads and plays the next track when one ends, and stops after the last", async () => {
    await useSetList.getState().add([3, 5]);
    await usePlayer.getState().load(3);
    usePlayer.getState().onPlaybackState(ended);
    await vi.waitFor(() => expect(usePlayer.getState().detail?.track.id).toBe(5));
    await vi.waitFor(() => expect(usePlayer.getState().playing).toBe(true));

    const lookup = vi.spyOn(mock, "trackDetail");
    usePlayer.getState().onPlaybackState(ended);
    expect(lookup).not.toHaveBeenCalled();
  });

  it("does not move on after a device change, or for a track outside the list", async () => {
    await useSetList.getState().add([3, 5]);
    await usePlayer.getState().load(3);
    const lookup = vi.spyOn(mock, "trackDetail");
    usePlayer.getState().onPlaybackState({ playing: false, ended: false });
    expect(lookup).not.toHaveBeenCalled();

    await usePlayer.getState().load(8);
    lookup.mockClear();
    usePlayer.getState().onPlaybackState(ended);
    expect(lookup).not.toHaveBeenCalled();
  });

  it("keeps a track picked just before the end", async () => {
    await useSetList.getState().add([3, 5]);
    await usePlayer.getState().load(3);
    const picking = usePlayer.getState().load(8); // still loading when 3 ends
    const lookup = vi.spyOn(mock, "trackDetail");
    usePlayer.getState().onPlaybackState(ended);
    expect(lookup).not.toHaveBeenCalledWith(5);
    await picking;
    expect(usePlayer.getState().detail?.track.id).toBe(8);
  });

  it("does not start the next track when the user pressed something during its load", async () => {
    await useSetList.getState().add([3, 5]);
    await usePlayer.getState().load(3);
    usePlayer.getState().onPlaybackState(ended);
    void usePlayer.getState().seek(0); // a press while 5 loads
    await vi.waitFor(() => expect(usePlayer.getState().detail?.track.id).toBe(5));
    await new Promise((resolve) => setTimeout(resolve, 0));
    expect(usePlayer.getState().playing).toBe(false);
  });

  it("never holds a track twice, even from overlapping adds", async () => {
    await useSetList.getState().add([4, 4]);
    await Promise.all([useSetList.getState().add([6]), useSetList.getState().add([6])]);
    expect(useSetList.getState().items.map((t) => t.id)).toEqual([4, 6]);
  });
});

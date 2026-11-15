import { beforeEach, describe, expect, it } from "vitest";

import { setApiForTests, type CratekitApi } from "@/lib/ipc";
import type { TrackDetail, TrackRow } from "@/lib/types";
import { createMockApi } from "@/mock/api";
import { DEFAULT_QUERY, useLibrary } from "@/stores/library";
import { usePlayer } from "@/stores/player";

function noop(): void {}

/** A promise resolved later by the test, to force out-of-order answers. */
function deferred<T>(): { promise: Promise<T>; resolve: (value: T) => void } {
  let settle: (value: T) => void = noop;
  const promise = new Promise<T>((resolve) => {
    settle = resolve;
  });
  return { promise, resolve: settle };
}

function row(id: number, title: string): TrackRow {
  return {
    id,
    title,
    artist: "Test",
    durationSec: 300,
    bpm: 124,
    camelot: "8A",
    keyName: "A minor",
    energy: 6,
    lufs: -9,
    analyzed: true,
    hasStems: false,
    mastered: false,
    activity: null,
  };
}

describe("stores ignore stale responses", () => {
  beforeEach(() => {
    useLibrary.setState({
      query: DEFAULT_QUERY,
      rows: [],
      status: "idle",
      selectedIds: [],
    });
    usePlayer.setState({ detail: null, playing: false });
  });

  it("library keeps the rows of the newest query when an older one resolves last", async () => {
    const mock = createMockApi();
    const slow = deferred<TrackRow[]>();
    const fast = deferred<TrackRow[]>();
    const answers = [slow.promise, fast.promise];
    const api: CratekitApi = {
      ...mock,
      libraryList: () => answers.shift() ?? Promise.resolve([]),
    };
    setApiForTests(api);

    const first = useLibrary.getState().selectCrate(1);
    const second = useLibrary.getState().selectCrate(2);
    fast.resolve([row(2, "Crate two")]);
    await second;
    slow.resolve([row(1, "Crate one")]);
    await first;

    expect(useLibrary.getState().query.crateId).toBe(2);
    expect(useLibrary.getState().rows.map((r) => r.title)).toEqual(["Crate two"]);
  });

  it("player shows the last clicked track even if an earlier load finishes later", async () => {
    const mock = createMockApi();
    const slowA = deferred<TrackDetail>();
    const detailB = await mock.trackDetail(2);
    const api: CratekitApi = {
      ...mock,
      trackDetail: (id) => (id === 1 ? slowA.promise : Promise.resolve(detailB)),
    };
    setApiForTests(api);

    const loadA = usePlayer.getState().load(1);
    await usePlayer.getState().load(2);
    slowA.resolve(await mock.trackDetail(1));
    await loadA;

    expect(usePlayer.getState().detail?.track.id).toBe(2);
  });
});

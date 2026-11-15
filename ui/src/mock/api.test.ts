import { afterEach, describe, expect, it, vi } from "vitest";

import type { CratekitApi } from "@/lib/ipc";
import { DEFAULT_QUERY } from "@/stores/library";
import { createMockApi } from "@/mock/api";
import { TRACK_COUNT } from "@/mock/fixtures";

/** Every member of the contract. If CratekitApi grows, this list must too. */
const MEMBERS: ReadonlyArray<keyof CratekitApi> = [
  "appVersion",
  "libraryAddPaths",
  "libraryList",
  "libraryCounts",
  "trackDetail",
  "cratesList",
  "crateCreate",
  "crateAddTracks",
  "crateRemoveTracks",
  "analyzeTracks",
  "jobCancel",
  "jobsSetPaused",
  "playbackLoad",
  "playbackPlay",
  "playbackPause",
  "playbackSeek",
  "playbackSetRate",
  "playbackSetMode",
  "stemsSplit",
  "stemsSetGain",
  "stemsSetMute",
  "stemsBackendStatus",
  "masterPreview",
  "masterExport",
  "hotcueSet",
  "hotcueClear",
  "midiListDevices",
  "midiLearnStart",
  "midiLearnCancel",
  "midiMappings",
  "exportVirtualdj",
  "modelsStatus",
  "modelsDownload",
  "modelsDelete",
  "audioListDevices",
  "audioSetDevice",
  "audioSetBufferSize",
  "settingsGet",
  "settingsSet",
  "onJobProgress",
  "onJobDone",
  "onJobFailed",
  "onPlaybackPosition",
  "onMeter",
  "onPlaybackState",
  "onMidiDeviceChanged",
  "onMidiLearned",
  "onControlEvent",
  "onAudioDeviceChanged",
];

describe("mock CratekitApi", () => {
  afterEach(() => {
    vi.useRealTimers();
  });

  it("implements every member of the contract", () => {
    const mock = createMockApi();
    for (const name of MEMBERS) expect(typeof mock[name], name).toBe("function");
  });

  it("lists the deterministic library and filters by crate", async () => {
    const mock = createMockApi();
    const all = await mock.libraryList(DEFAULT_QUERY);
    expect(all).toHaveLength(TRACK_COUNT);
    const [first] = await mock.cratesList();
    expect(first).toBeDefined();
    const inCrate = await mock.libraryList({
      ...DEFAULT_QUERY,
      crateId: first?.id ?? null,
    });
    expect(inCrate.length).toBe(first?.trackCount);
  });

  it("emits job progress and completion for analysis", async () => {
    vi.useFakeTimers();
    const mock = createMockApi();
    const progress = vi.fn();
    const done = vi.fn();
    await mock.onJobProgress(progress);
    await mock.onJobDone(done);
    await mock.analyzeTracks([1, 2, 3]);
    vi.advanceTimersByTime(400 * 3);
    expect(progress).toHaveBeenCalledWith(
      expect.objectContaining({ kind: "analysis", done: 3, total: 3 }),
    );
    expect(done).toHaveBeenCalledWith(expect.objectContaining({ kind: "analysis" }));
  });

  it("streams the playhead at about 30 Hz while playing", async () => {
    vi.useFakeTimers();
    const mock = createMockApi();
    const positions: number[] = [];
    await mock.onPlaybackPosition((sec) => positions.push(sec));
    await mock.playbackLoad(1);
    await mock.playbackPlay();
    vi.advanceTimersByTime(1000);
    await mock.playbackPause();
    expect(positions.length).toBeGreaterThanOrEqual(29);
    expect(positions.at(-1)).toBeCloseTo(1, 0);
  });
});

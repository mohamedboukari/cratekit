import { afterEach, describe, expect, it, vi } from "vitest";

import type { CratekitApi } from "@/lib/ipc";
import { DEFAULT_QUERY } from "@/stores/library";
import { createMockApi } from "@/mock/api";
import { TRACK_COUNT } from "@/mock/fixtures";

/** Every member of the contract. If CratekitApi grows, this list must too. */
const MEMBERS: ReadonlyArray<keyof CratekitApi> = [
  "appVersion",
  "appOpenSettings",
  "dialogOpenFiles",
  "dialogOpenFolder",
  "libraryAddPaths",
  "libraryList",
  "libraryCounts",
  "trackDetail",
  "cratesList",
  "crateCreate",
  "crateAddTracks",
  "crateRemoveTracks",
  "setlistTransitions",
  "analyzeTracks",
  "jobCancel",
  "jobsSetPaused",
  "playbackLoad",
  "playbackPlay",
  "playbackPause",
  "playbackSeek",
  "playbackSetRate",
  "playbackSetMode",
  "playbackSetFilter",
  "playbackSetLevelMatch",
  "stemsSplit",
  "stemsSetGain",
  "stemsSetMute",
  "stemsSetSolo",
  "stemsBackendStatus",
  "stemsInfo",
  "stemsExport",
  "masterPresets",
  "masterPreview",
  "masterExport",
  "hotcueSet",
  "hotcueClear",
  "hotcueUpdate",
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
  "onFilesDropped",
  "onSettingsChanged",
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

describe("mock rules that stand in for Rust", () => {
  it("refuses to split stems before the model is downloaded", async () => {
    const mock = createMockApi();
    await expect(mock.stemsSplit(3)).rejects.toMatchObject({ code: "model_missing" });
  });

  it("marks the key's scale in the chroma and reports unknown keys as null", async () => {
    const mock = createMockApi();
    const detail = await mock.trackDetail(1);
    expect(detail.analysis?.chromaInKey.filter(Boolean)).toHaveLength(7);
    const unanalyzed = await mock.setlistTransitions([9, 1]);
    expect(unanalyzed[0]?.keyCompatible).toBeNull();
  });
});

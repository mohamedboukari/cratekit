import { beforeEach, describe, expect, it, vi } from "vitest";

import { setApiForTests, type CratekitApi } from "@/lib/ipc";
import type { StemName } from "@/lib/types";
import { createMockApi } from "@/mock/api";
import { useController } from "@/stores/controller";
import { useLibrary } from "@/stores/library";
import { useMaster } from "@/stores/master";
import { useStems } from "@/stores/stems";

describe("master store", () => {
  beforeEach(() => setApiForTests(createMockApi()));

  it("takes the loudness target from Rust's preset when a preset is picked", async () => {
    await useMaster.getState().loadPresets();
    useMaster.getState().setPreset("chill");
    expect(useMaster.getState().settings).toMatchObject({
      preset: "chill",
      targetLufs: -12,
    });
    useMaster.getState().setTargetLufs(-10.5);
    expect(useMaster.getState().settings.targetLufs).toBe(-10.5);
  });

  it("remembers which track an export belongs to", async () => {
    const result = await useMaster.getState().exportMaster(3);
    expect(result?.path).toMatch(/\/Masters\/.+ master\)\.wav$/);
    expect(result?.targetReached).toBe(true);
    expect(useMaster.getState().result?.trackId).toBe(3);
    expect(useMaster.getState().exporting).toBe(false);
  });

  it("refreshes the browser after an export, so the track shows as mastered", async () => {
    await useLibrary.getState().load();
    const before = useLibrary.getState().rows.find((row) => row.id === 4);
    expect(before?.mastered).toBe(false);
    await useMaster.getState().exportMaster(4);
    await vi.waitFor(() =>
      expect(useLibrary.getState().rows.find((row) => row.id === 4)?.mastered).toBe(true),
    );
  });
});

describe("stems store", () => {
  const calls: Array<[StemName, boolean]> = [];

  beforeEach(() => {
    calls.length = 0;
    const mock = createMockApi();
    const api: CratekitApi = {
      ...mock,
      stemsSetMute: (stem, muted) => {
        calls.push([stem, muted]);
        return mock.stemsSetMute(stem, muted);
      },
    };
    setApiForTests(api);
    useStems.setState({ trackId: null, info: null, status: "idle" });
  });

  it("mutes every stem of the instrumental group together", async () => {
    await useStems.getState().toggleMute(["drums", "bass", "other"]);
    expect(calls).toEqual([
      ["drums", true],
      ["bass", true],
      ["other", true],
    ]);
    expect(useStems.getState().muted).toMatchObject({
      drums: true,
      bass: true,
      other: true,
    });
    expect(useStems.getState().muted.vocals).toBe(false);
  });

  it("starts a new track with a neutral mix and reports the missing model", async () => {
    await useStems.getState().toggleMute(["vocals"]);
    await useStems.getState().load(3);
    expect(useStems.getState().muted.vocals).toBe(false);
    expect(useStems.getState().modelReady).toBe(false);
    // Track 3 has stems in the fixtures.
    expect(useStems.getState().info?.waveforms.vocals.peaks.length).toBeGreaterThan(0);
  });
});

describe("controller store", () => {
  beforeEach(() => {
    setApiForTests(createMockApi());
    useController.setState({ mappings: [], learning: null, lastLearned: null });
  });

  it("lists every control target and moves a learned binding away from the old target", async () => {
    await useController.getState().load("ddj-flx4");
    const mappings = useController.getState().mappings;
    expect(mappings.length).toBeGreaterThan(15);
    const playBinding = mappings.find((m) => m.target === "play")?.binding ?? null;
    expect(playBinding).not.toBeNull();
    if (!playBinding) return;
    useController.setState({ learning: "tempo" });
    useController
      .getState()
      .onLearned({ target: "tempo", binding: playBinding, replaced: "play" });
    const after = useController.getState().mappings;
    expect(after.find((m) => m.target === "tempo")?.binding).toEqual(playBinding);
    expect(after.find((m) => m.target === "play")?.binding).toBeNull();
    expect(useController.getState().learning).toBeNull();
  });
});

import { beforeEach, describe, expect, it } from "vitest";

import { DEFAULT_SETTINGS } from "@/lib/defaults";
import { setApiForTests, type CratekitApi } from "@/lib/ipc";
import { createMockApi } from "@/mock/api";
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
});

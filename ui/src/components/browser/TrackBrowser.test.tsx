import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { TrackBrowser } from "@/components/browser/TrackBrowser";
import { setApiForTests } from "@/lib/ipc";
import { createMockApi } from "@/mock/api";
import { DEFAULT_QUERY, useLibrary } from "@/stores/library";
import { usePlayer } from "@/stores/player";

/** Renders, loads the mock library and waits for the virtualizer's first window of rows. */
async function renderLoaded(): Promise<HTMLElement> {
  render(<TrackBrowser />);
  await useLibrary.getState().load();
  const grid = await screen.findByRole("grid");
  // The virtualizer measures in a layout effect and renders rows one render later.
  await waitFor(() => expect(within(grid).getAllByRole("row").length).toBeGreaterThan(1));
  return grid;
}

describe("TrackBrowser", () => {
  beforeEach(() => {
    setApiForTests(createMockApi());
    useLibrary.setState({
      query: DEFAULT_QUERY,
      rows: [],
      status: "idle",
      selectedIds: [],
    });
    usePlayer.setState({ detail: null, playing: false });
    // jsdom has no layout (every element is 0x0); give the scroll area a size
    // so the virtualizer renders a window of rows.
    // (@tanstack/virtual-core reads clientWidth/clientHeight of the scroll element.)
    vi.spyOn(HTMLElement.prototype, "clientHeight", "get").mockReturnValue(480);
    vi.spyOn(HTMLElement.prototype, "clientWidth", "get").mockReturnValue(900);
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  it("renders a virtualized slice of the library, not every row", async () => {
    const grid = await renderLoaded();
    const rows = within(grid).getAllByRole("row");
    // 1 header row + the visible window (60 tracks in the fixtures).
    expect(rows.length).toBeGreaterThan(5);
    expect(rows.length).toBeLessThan(61);
    expect(grid).toHaveAttribute("aria-rowcount", "61");
  });

  it("sorts by BPM when the header is clicked", async () => {
    await renderLoaded();
    fireEvent.click(screen.getByRole("button", { name: "BPM" }));
    expect(screen.getByRole("columnheader", { name: "BPM" })).toHaveAttribute(
      "aria-sort",
      "ascending",
    );
    const bpms = useLibrary
      .getState()
      .rows.map((r) => r.bpm)
      .filter((b): b is number => b !== null)
      .toSorted((a, b) => a - b);
    const firstRow = screen.getAllByRole("row")[1];
    expect(firstRow).toHaveTextContent(bpms[0]?.toFixed(1) ?? "");
  });

  it("moves the active row and selection with the arrow keys", async () => {
    const grid = await renderLoaded();
    fireEvent.keyDown(grid, { key: "ArrowDown" });
    const selected = useLibrary.getState().selectedIds;
    expect(selected).toHaveLength(1);
    expect(grid).toHaveAttribute("aria-activedescendant", `track-row-${selected[0]}`);
    fireEvent.keyDown(grid, { key: "ArrowDown", shiftKey: true });
    expect(useLibrary.getState().selectedIds).toHaveLength(2);
  });

  it("keeps the active row on the same track after a sort", async () => {
    const grid = await renderLoaded();
    fireEvent.keyDown(grid, { key: "ArrowDown" });
    const active = grid.getAttribute("aria-activedescendant");
    fireEvent.click(screen.getByRole("button", { name: "BPM" }));
    expect(grid).toHaveAttribute("aria-activedescendant", active);
  });

  it("drops hidden tracks from the selection when the search changes", async () => {
    await renderLoaded();
    useLibrary.getState().select(useLibrary.getState().rows.map((r) => r.id));
    await useLibrary.getState().setQuery({ hasStems: true });
    const visible = new Set(useLibrary.getState().rows.map((r) => r.id));
    expect(useLibrary.getState().selectedIds.every((id) => visible.has(id))).toBe(true);
    expect(useLibrary.getState().selectedIds.length).toBe(visible.size);
  });
});

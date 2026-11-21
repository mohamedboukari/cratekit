import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { DragGhost } from "@/components/browser/DragGhost";
import { SetList } from "@/components/browser/SetList";
import { TrackBrowser } from "@/components/browser/TrackBrowser";
import { DEFAULT_SETTINGS } from "@/lib/defaults";
import { setApiForTests } from "@/lib/ipc";
import { createMockApi } from "@/mock/api";
import { useDrag } from "@/stores/drag";
import { DEFAULT_QUERY, useLibrary } from "@/stores/library";
import { usePlayer } from "@/stores/player";
import { useSetList } from "@/stores/setlist";
import { useSettings } from "@/stores/settings";

/** jsdom has no layout: the test says which element is under the pointer. */
let under: Element | null = null;

function pointAt(element: Element | null): void {
  under = element;
}

/** Presses on `source` and moves well past the threshold over `target`, button held. */
function pressAndMove(source: Element, target: Element | null): void {
  fireEvent.pointerDown(source, { button: 0, buttons: 1, clientX: 10, clientY: 10 });
  pointAt(target);
  fireEvent.pointerMove(window, { buttons: 1, clientX: 40, clientY: 60 });
  fireEvent.pointerMove(window, { buttons: 1, clientX: 60, clientY: 90 });
}

/** A whole drag: press on `source`, move over `target`, release. */
function drag(source: Element, target: Element | null): void {
  pressAndMove(source, target);
  fireEvent.pointerUp(window, { clientX: 60, clientY: 90 });
}

/** Lets pending store work (async adds and moves) finish. */
async function settle(): Promise<void> {
  await act(async () => {});
}

async function renderBrowserAndSetList(): Promise<HTMLElement> {
  render(
    <>
      <TrackBrowser />
      <SetList />
      <DragGhost />
    </>,
  );
  await useLibrary.getState().load();
  const grid = await screen.findByRole("grid");
  await waitFor(() => expect(within(grid).getAllByRole("row").length).toBeGreaterThan(3));
  return grid;
}

/** The track row with this id (rows carry `data-track-id`). */
function row(grid: HTMLElement, id: number): Element {
  const found = grid.querySelector(`tr[data-track-id="${id}"]`);
  if (!found) throw new Error(`row ${id} is not rendered`);
  return found;
}

function slot(index: number): Element | null {
  return document.querySelector(`[data-drop-index="${index}"]`);
}

describe("in-app drags run on pointer events (Tauri's file drop takes HTML5 drags)", () => {
  beforeEach(() => {
    setApiForTests(createMockApi());
    useSettings.setState({
      settings: structuredClone(DEFAULT_SETTINGS),
      status: "ready",
    });
    useLibrary.setState({
      query: DEFAULT_QUERY,
      rows: [],
      status: "idle",
      selectedIds: [],
    });
    usePlayer.setState({ detail: null, playing: false });
    useSetList.setState({ items: [], transitions: [] });
    useDrag.setState({ payload: null, over: null });
    under = null;
    Object.defineProperty(document, "elementFromPoint", {
      configurable: true,
      value: () => under,
    });
    vi.spyOn(HTMLElement.prototype, "clientHeight", "get").mockReturnValue(480);
    vi.spyOn(HTMLElement.prototype, "clientWidth", "get").mockReturnValue(900);
  });

  afterEach(() => {
    vi.restoreAllMocks();
    Reflect.deleteProperty(document, "elementFromPoint");
  });

  it("adds a track dragged from the browser onto the empty set list", async () => {
    const grid = await renderBrowserAndSetList();
    drag(row(grid, 2), slot(0));
    await waitFor(() =>
      expect(useSetList.getState().items.map((t) => t.id)).toEqual([2]),
    );
    expect(useDrag.getState().payload).toBeNull();
    expect(document.body).not.toHaveClass("ck-dragging");
  });

  it("drags the whole selection when it starts on a selected row", async () => {
    const grid = await renderBrowserAndSetList();
    act(() => useLibrary.getState().select([3, 5]));
    drag(row(grid, 5), slot(0));
    await waitFor(() =>
      expect(useSetList.getState().items.map((t) => t.id)).toEqual([3, 5]),
    );
  });

  it("shows the drop line and the label while dragging, and Escape cancels", async () => {
    const grid = await renderBrowserAndSetList();
    act(() => useLibrary.getState().select([3, 5]));
    pressAndMove(row(grid, 3), slot(0));
    expect(useDrag.getState().over).toBe(0);
    expect(await screen.findByText("2 tracks")).toBeVisible();
    expect(slot(0)).toHaveClass("border-primary");
    expect(document.body).toHaveClass("ck-dragging");

    fireEvent.keyDown(window, { key: "Escape" });
    // Cancelled at once, before any release.
    expect(useDrag.getState().payload).toBeNull();
    expect(document.body).not.toHaveClass("ck-dragging");
    expect(slot(0)).not.toHaveClass("border-primary");
    fireEvent.pointerUp(window, { clientX: 60, clientY: 90 });
    await settle();
    expect(useSetList.getState().items).toEqual([]);
  });

  it("drags only the pressed row when it is not part of the selection", async () => {
    const grid = await renderBrowserAndSetList();
    act(() => useLibrary.getState().select([3, 5]));
    drag(row(grid, 7), slot(0));
    await waitFor(() =>
      expect(useSetList.getState().items.map((t) => t.id)).toEqual([7]),
    );
  });

  it("ends a press whose release never arrives (button up elsewhere, or a context menu)", async () => {
    const grid = await renderBrowserAndSetList();
    pressAndMove(row(grid, 2), slot(0));
    expect(useDrag.getState().payload).not.toBeNull();
    // The next move reports no button down: the release happened where we could not see it.
    fireEvent.pointerMove(window, { buttons: 0, clientX: 70, clientY: 95 });
    expect(useDrag.getState().payload).toBeNull();
    expect(document.body).not.toHaveClass("ck-dragging");

    // Control+click on macOS: the context menu opens at the press; no drag may start.
    fireEvent.pointerDown(row(grid, 2), {
      button: 0,
      buttons: 1,
      clientX: 10,
      clientY: 10,
    });
    fireEvent.contextMenu(row(grid, 2));
    fireEvent.pointerMove(window, { buttons: 1, clientX: 60, clientY: 90 });
    expect(useDrag.getState().payload).toBeNull();
    fireEvent.pointerUp(window, { clientX: 60, clientY: 90 });
    await settle();
    expect(useSetList.getState().items).toEqual([]);
  });

  it("keeps a click a click: a press that barely moves selects the row and drags nothing", async () => {
    const grid = await renderBrowserAndSetList();
    const target = row(grid, 4);
    fireEvent.pointerDown(target, { button: 0, buttons: 1, clientX: 10, clientY: 10 });
    pointAt(slot(0));
    fireEvent.pointerMove(window, { buttons: 1, clientX: 12, clientY: 11 });
    fireEvent.pointerUp(window, { clientX: 12, clientY: 11 });
    fireEvent.click(target);
    await settle();
    expect(useDrag.getState().payload).toBeNull();
    expect(useSetList.getState().items).toEqual([]);
    expect(useLibrary.getState().selectedIds).toEqual([4]);
  });

  it("reorders the set list by dragging an item to another slot", async () => {
    await renderBrowserAndSetList();
    await act(() => useSetList.getState().add([1, 2, 3]));
    const first = await screen.findByRole("button", { name: /^1\. / });
    drag(first, slot(3)); // the end slot
    await waitFor(() =>
      expect(useSetList.getState().items.map((t) => t.id)).toEqual([2, 3, 1]),
    );
  });

  it("moves an item up the list", async () => {
    await renderBrowserAndSetList();
    await act(() => useSetList.getState().add([1, 2, 3]));
    const third = await screen.findByRole("button", { name: /^3\. / });
    drag(third, slot(0));
    await waitFor(() =>
      expect(useSetList.getState().items.map((t) => t.id)).toEqual([3, 1, 2]),
    );
  });

  it("moves the dragged track even when the list changes during the drag", async () => {
    await renderBrowserAndSetList();
    await act(() => useSetList.getState().add([1, 2, 3]));
    const second = await screen.findByRole("button", { name: /^2\. / });
    pressAndMove(second, null);
    await act(() => useSetList.getState().remove(1)); // now [2, 3]
    pointAt(slot(2)); // the end slot of the shorter list
    fireEvent.pointerMove(window, { buttons: 1, clientX: 70, clientY: 95 });
    fireEvent.pointerUp(window, { clientX: 70, clientY: 95 });
    await waitFor(() =>
      expect(useSetList.getState().items.map((t) => t.id)).toEqual([3, 2]),
    );
  });

  it("does not load the track when a reorder drag is released over its own item", async () => {
    const load = vi.fn((_id: number) => Promise.resolve());
    const original = usePlayer.getState().load;
    usePlayer.setState({ load });
    try {
      await renderBrowserAndSetList();
      await act(() => useSetList.getState().add([1, 2]));
      const first = await screen.findByRole("button", { name: /^1\. / });
      drag(first, slot(0));
      // The browser sends a click after pointerup on the same element; it must not load.
      fireEvent.click(first);
      expect(load).not.toHaveBeenCalled();
      expect(useSetList.getState().items.map((t) => t.id)).toEqual([1, 2]);
      // A plain click still loads.
      fireEvent.click(first);
      expect(load).toHaveBeenCalledWith(1);
    } finally {
      usePlayer.setState({ load: original });
    }
  });
});

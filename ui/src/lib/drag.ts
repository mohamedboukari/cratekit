import type { PointerEvent as ReactPointerEvent } from "react";

import { useDrag, type DragPayload } from "@/stores/drag";

/**
 * In-app drags (browser rows to the set list, set list reordering) run on
 * pointer events, not HTML5 drag and drop: on macOS and Windows, Tauri's
 * native file drop takes every drag over the webview, so HTML5 drop events
 * never reach the page. Pointer events always do, and files dropped from the
 * system keep working (lib/ipc.tauri.ts, onFilesDropped).
 *
 * Drop targets are elements with `data-drop-index`; one handler (the set
 * list's) decides what a drop does.
 */

/** How far (px) the pointer moves before a press becomes a drag, so clicks stay clicks. */
const DRAG_THRESHOLD = 4;

/** While dragging: the grabbing cursor everywhere (index.css; app text is never selectable). */
const DRAGGING_CLASS = "ck-dragging";

type DropHandler = (payload: DragPayload, index: number) => void;
type MoveListener = (x: number, y: number) => void;

let dropHandler: DropHandler | null = null;
const moveListeners = new Set<MoveListener>();
/** Ends the press or drag being watched; only one at a time. */
let endActive: (() => void) | null = null;

/** Sets what a drop does; returns a function that removes it. */
export function setDropHandler(handler: DropHandler): () => void {
  dropHandler = handler;
  return () => {
    if (dropHandler === handler) dropHandler = null;
  };
}

/** Calls `listener` with the pointer position on every move of a drag (outside React renders). */
export function onDragMove(listener: MoveListener): () => void {
  moveListeners.add(listener);
  return () => {
    moveListeners.delete(listener);
  };
}

/** The drop slot at a point: the nearest element with `data-drop-index`. */
function slotAt(x: number, y: number): number | null {
  const slot = document.elementFromPoint(x, y)?.closest<HTMLElement>("[data-drop-index]");
  if (!slot) return null;
  const index = Number(slot.dataset.dropIndex);
  return Number.isInteger(index) && index >= 0 ? index : null;
}

/** Within this distance (px) of a drop area's top or bottom edge, a drag scrolls it. */
const EDGE_PX = 28;
/** Fastest auto-scroll (px per frame), right at the edge. */
const MAX_SCROLL_PX = 14;

interface EdgeScroll {
  area: HTMLElement;
  /** Pixels per frame: negative scrolls up, positive down. */
  speed: number;
}

/** The drop area (`data-drop-scroll`) to scroll when the pointer is near its edge, and how fast. */
function edgeScrollAt(x: number, y: number): EdgeScroll | null {
  const area = document
    .elementFromPoint(x, y)
    ?.closest<HTMLElement>("[data-drop-scroll]");
  if (!area) return null;
  const rect = area.getBoundingClientRect();
  const fromTop = y - rect.top;
  const fromBottom = rect.bottom - y;
  if (fromTop >= 0 && fromTop < EDGE_PX)
    return { area, speed: -MAX_SCROLL_PX * (1 - fromTop / EDGE_PX) };
  if (fromBottom >= 0 && fromBottom < EDGE_PX)
    return { area, speed: MAX_SCROLL_PX * (1 - fromBottom / EDGE_PX) };
  return null;
}

function swallow(event: MouseEvent): void {
  event.stopPropagation();
  event.preventDefault();
}

/** Swallows the click (and double-click) the browser sends after a drag released over its source. */
function swallowNextClick(): void {
  window.addEventListener("click", swallow, { capture: true, once: true });
  window.addEventListener("dblclick", swallow, { capture: true, once: true });
  setTimeout(() => {
    window.removeEventListener("click", swallow, { capture: true });
    window.removeEventListener("dblclick", swallow, { capture: true });
  }, 0);
}

/**
 * Watches a press on a drag source. It becomes a drag once the pointer moves
 * past the threshold (`payload` is asked for then, so it sees the current
 * selection); releasing over a drop slot calls the drop handler. Escape, a
 * cancelled pointer, a lost button, the window losing focus or a context menu
 * end it without a drop. Near the top or bottom of a drop area, the area scrolls.
 */
export function pressToDrag(
  event: ReactPointerEvent<HTMLElement>,
  payload: () => DragPayload | null,
): void {
  if (event.button !== 0) return;
  endActive?.();
  const startX = event.clientX;
  const startY = event.clientY;
  let lastX = startX;
  let lastY = startY;
  let active: DragPayload | null = null;
  let scroll: EdgeScroll | null = null;
  let frame: number | null = null;

  function end(): void {
    window.removeEventListener("pointermove", move);
    window.removeEventListener("pointerup", up);
    window.removeEventListener("pointercancel", end);
    window.removeEventListener("blur", end);
    window.removeEventListener("contextmenu", end, true);
    window.removeEventListener("keydown", key, true);
    window.removeEventListener("scroll", followScroll, true);
    if (frame !== null) cancelAnimationFrame(frame);
    frame = null;
    scroll = null;
    document.body.classList.remove(DRAGGING_CLASS);
    if (active !== null) useDrag.setState({ payload: null, over: null });
    active = null;
    endActive = null;
  }

  function updateOver(): void {
    const over = slotAt(lastX, lastY);
    if (over !== useDrag.getState().over) useDrag.setState({ over });
  }

  /** Scrolling moves the slots under a still pointer: the drop line follows. */
  function followScroll(): void {
    if (active !== null) updateOver();
  }

  function tick(): void {
    frame = null;
    if (active === null || scroll === null) return;
    scroll.area.scrollTop += scroll.speed;
    updateOver();
    frame = requestAnimationFrame(tick);
  }

  function move(e: PointerEvent): void {
    // The button was released where no pointerup reached us (another window, a menu).
    if ((e.buttons & 1) === 0) {
      end();
      return;
    }
    lastX = e.clientX;
    lastY = e.clientY;
    if (active === null) {
      if (Math.hypot(lastX - startX, lastY - startY) < DRAG_THRESHOLD) return;
      active = payload();
      if (active === null) {
        end();
        return;
      }
      document.body.classList.add(DRAGGING_CLASS);
      useDrag.setState({ payload: active, over: null });
    }
    for (const listener of moveListeners) listener(lastX, lastY);
    updateOver();
    scroll = edgeScrollAt(lastX, lastY);
    if (scroll !== null && frame === null) frame = requestAnimationFrame(tick);
  }

  function up(e: PointerEvent): void {
    const dropped = active;
    const over = dropped === null ? null : slotAt(e.clientX, e.clientY);
    end();
    if (dropped === null) return;
    swallowNextClick();
    if (over !== null) dropHandler?.(dropped, over);
  }

  function key(e: KeyboardEvent): void {
    if (e.key !== "Escape" || active === null) return;
    // Escape cancels the drag only; deck shortcuts and dialogs never see it.
    e.preventDefault();
    e.stopPropagation();
    end();
  }

  window.addEventListener("pointermove", move);
  window.addEventListener("pointerup", up);
  window.addEventListener("pointercancel", end);
  // Alt+Tab mid-press, or a context menu (Control+click on macOS) opening;
  // capture, because the menu trigger stops the event before it bubbles.
  window.addEventListener("blur", end);
  window.addEventListener("contextmenu", end, true);
  window.addEventListener("keydown", key, true);
  window.addEventListener("scroll", followScroll, true);
  endActive = end;
}

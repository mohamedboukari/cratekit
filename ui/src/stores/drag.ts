import { create } from "zustand";

import type { TrackId } from "@/lib/types";

/** What an in-app drag carries: tracks from the browser, or a set list item being moved. */
export type DragPayload =
  | { kind: "tracks"; ids: TrackId[]; label: string }
  /** A set list item, by id: its index is looked up at the drop, so changes meanwhile are safe. */
  | { kind: "reorder"; id: TrackId; label: string };

/** The in-app drag in progress (lib/drag.ts runs it). */
interface DragState {
  /** Set once the pointer has moved past the threshold; null otherwise. */
  payload: DragPayload | null;
  /** The drop slot under the pointer (its `data-drop-index`), or null. */
  over: number | null;
}

export const useDrag = create<DragState>()(() => ({ payload: null, over: null }));

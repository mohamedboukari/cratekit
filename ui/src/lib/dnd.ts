import type { TrackId } from "@/lib/types";

/** Drag-and-drop payload type for track ids (browser rows to the set list). */
export const TRACK_DRAG_MIME = "application/x-cratekit-tracks";

/** Reads dropped track ids; ignores anything that is not a list of numbers. */
export function parseTrackDrop(raw: string): TrackId[] {
  if (!raw) return [];
  try {
    const value: unknown = JSON.parse(raw);
    if (!Array.isArray(value)) return [];
    return value.filter((x): x is number => typeof x === "number" && Number.isInteger(x));
  } catch {
    return [];
  }
}

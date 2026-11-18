import {
  createColumnHelper,
  createSortedRowModel,
  rowSortingFeature,
  tableFeatures,
} from "@tanstack/react-table";

import type { TrackRow } from "@/lib/types";

/** Table features, at module scope so the table never re-initializes. */
export const browserFeatures = tableFeatures({
  rowSortingFeature,
  sortedRowModel: createSortedRowModel(),
});

const helper = createColumnHelper<typeof browserFeatures, TrackRow>();

interface SortableRow {
  original: TrackRow;
}

/** Nullable numbers sort with missing values (not analyzed) last. */
function byNumber(pick: (t: TrackRow) => number | null) {
  return (a: SortableRow, b: SortableRow): number => {
    const x = pick(a.original);
    const y = pick(b.original);
    if (x === null && y === null) return 0;
    if (x === null) return 1;
    if (y === null) return -1;
    return x - y;
  };
}

function byText(pick: (t: TrackRow) => string) {
  return (a: SortableRow, b: SortableRow): number =>
    pick(a.original).localeCompare(pick(b.original), undefined, {
      sensitivity: "base",
      numeric: true,
    });
}

/** Camelot codes sort by wheel number then letter (2A before 10A); missing last. */
function camelotRank(code: string | null): number | null {
  if (!code) return null;
  const num = Number.parseInt(code, 10);
  return Number.isNaN(num) ? null : num * 2 + (code.endsWith("B") ? 1 : 0);
}

/**
 * Sortable columns. Rendering (#, ready tags, energy bars) lives in TrackBrowser.
 * Headers are translation keys; TrackBrowser translates them at render.
 */
export const browserColumns = helper.columns([
  helper.accessor("title", {
    header: "browser.columns.title",
    sortFn: byText((t) => t.title),
  }),
  helper.accessor("artist", {
    header: "browser.columns.artist",
    sortFn: byText((t) => t.artist),
  }),
  helper.accessor("bpm", {
    header: "browser.columns.bpm",
    sortFn: byNumber((t) => t.bpm),
  }),
  helper.accessor("camelot", {
    header: "browser.columns.key",
    sortFn: byNumber((t) => camelotRank(t.camelot)),
  }),
  helper.accessor("energy", {
    header: "browser.columns.energy",
    sortFn: byNumber((t) => t.energy),
  }),
  helper.accessor("durationSec", {
    header: "browser.columns.time",
    sortFn: byNumber((t) => t.durationSec),
  }),
]);

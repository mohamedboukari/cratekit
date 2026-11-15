import { create } from "zustand";

import { api } from "@/lib/ipc";
import { notifyError } from "@/lib/notify";
import type {
  Crate,
  CrateId,
  LibraryCounts,
  LibraryQuery,
  TrackId,
  TrackRow,
} from "@/lib/types";

export const DEFAULT_QUERY: LibraryQuery = {
  crateId: null,
  search: "",
  bpmMin: null,
  bpmMax: null,
  nearBpm: null,
  matchCamelot: null,
  hasStems: false,
};

/**
 * Rust commands are async and can resolve out of order (a slow query for crate 1
 * after a fast one for crate 2). Every request takes a number; only the newest
 * one may write rows.
 */
let requestSeq = 0;

type Status = "idle" | "loading" | "ready" | "error";

interface LibraryState {
  query: LibraryQuery;
  rows: TrackRow[];
  crates: Crate[];
  counts: LibraryCounts | null;
  status: Status;
  selectedIds: TrackId[];
  /** Loads crates, counts and the rows for the current query. */
  load: () => Promise<void>;
  /** Re-runs the current query (after a job changes track data). */
  refreshRows: () => Promise<void>;
  setQuery: (patch: Partial<LibraryQuery>) => Promise<void>;
  selectCrate: (crateId: CrateId | null) => Promise<void>;
  select: (ids: TrackId[]) => void;
}

export const useLibrary = create<LibraryState>()((set, get) => ({
  query: DEFAULT_QUERY,
  rows: [],
  crates: [],
  counts: null,
  status: "idle",
  selectedIds: [],

  load: async () => {
    const seq = ++requestSeq;
    set({ status: "loading" });
    try {
      const [crates, counts, rows] = await Promise.all([
        api().cratesList(),
        api().libraryCounts(),
        api().libraryList(get().query),
      ]);
      if (seq !== requestSeq) return;
      set({ crates, counts, rows, status: "ready" });
    } catch (error) {
      if (seq !== requestSeq) return;
      set({ status: "error" });
      notifyError(error, "Could not load the library");
    }
  },

  refreshRows: async () => {
    const seq = ++requestSeq;
    try {
      const [rows, counts, crates] = await Promise.all([
        api().libraryList(get().query),
        api().libraryCounts(),
        api().cratesList(),
      ]);
      if (seq !== requestSeq) return;
      // Hidden tracks leave the selection, so actions only apply to what is visible.
      const visible = new Set(rows.map((r) => r.id));
      const selectedIds = get().selectedIds.filter((id) => visible.has(id));
      set({ rows, counts, crates, status: "ready", selectedIds });
    } catch (error) {
      if (seq !== requestSeq) return;
      notifyError(error, "Could not refresh the library");
    }
  },

  setQuery: async (patch) => {
    set({ query: { ...get().query, ...patch } });
    await get().refreshRows();
  },

  selectCrate: async (crateId) => {
    set({ selectedIds: [] });
    await get().setQuery({ crateId });
  },

  select: (ids) => set({ selectedIds: ids }),
}));

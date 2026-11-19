import { create } from "zustand";

import { i18n } from "@/i18n";
import { api, toAppError } from "@/lib/ipc";
import { notifyError } from "@/lib/notify";
import type { TrackId, TrackRow, Transition } from "@/lib/types";
import { useSettings } from "@/stores/settings";

/** Only the newest change may write transitions (Rust answers can overlap). */
let changeSeq = 0;
/** Edits wait for the saved list to load, so they never overwrite it with a partial list. */
let loading: Promise<void> = Promise.resolve();
/** True once `items` reflects the saved list (or the list was empty). */
let loaded = false;

/**
 * The set builder side list (#55): an ordered list of tracks to play. The
 * order persists in settings (`setList`). How neighbours fit together (key
 * and BPM jumps) is computed by Rust (`setlistTransitions`).
 */
interface SetListState {
  items: TrackRow[];
  transitions: Transition[];
  /** Loads the saved list from settings (call after settings loaded). */
  load: () => Promise<void>;
  /** Re-reads the rows and transitions (after analysis changes track data). */
  refresh: () => Promise<void>;
  /** Appends tracks (or inserts at `index`); duplicates are ignored. */
  add: (ids: TrackId[], index?: number) => Promise<void>;
  move: (from: number, to: number) => Promise<void>;
  remove: (id: TrackId) => Promise<void>;
  clear: () => Promise<void>;
  /** Saves the list, in order, as a new crate. */
  saveAsCrate: (name: string) => Promise<boolean>;
}

/**
 * Rows for the ids, in order. Tracks Rust no longer knows (`not_found`) are
 * left out; any other failure rejects, so a hiccup never prunes the list.
 */
async function rowsFor(ids: TrackId[]): Promise<TrackRow[]> {
  const results = await Promise.allSettled(ids.map((id) => api().trackDetail(id)));
  const rows: TrackRow[] = [];
  for (const result of results) {
    if (result.status === "fulfilled") rows.push(result.value.track);
    else if (toAppError(result.reason).code !== "not_found") throw result.reason;
  }
  return rows;
}

export const useSetList = create<SetListState>()((set, get) => {
  /** Asks Rust how neighbours fit; only the newest request may write. */
  async function updateTransitions(ids: TrackId[], seq: number): Promise<void> {
    const transitions = ids.length > 1 ? await api().setlistTransitions(ids) : [];
    if (seq === changeSeq) set({ transitions });
  }

  /** Persists the order and refreshes the transitions for it. */
  async function commit(items: TrackRow[]): Promise<void> {
    const seq = ++changeSeq;
    const ids = items.map((t) => t.id);
    set({ items });
    try {
      await useSettings.getState().set("setList", ids);
      await updateTransitions(ids, seq);
    } catch (error) {
      if (seq === changeSeq) notifyError(error, i18n.t("errors.setlist.update"));
    }
  }

  /** Reads rows for the ids; prunes the saved list only when tracks are gone. */
  async function reload(ids: TrackId[]): Promise<void> {
    const items = await rowsFor(ids);
    loaded = true;
    if (items.length < ids.length) {
      await commit(items);
      return;
    }
    const seq = ++changeSeq;
    set({ items });
    try {
      await updateTransitions(ids, seq);
    } catch (error) {
      if (seq === changeSeq) notifyError(error, i18n.t("errors.setlist.check"));
    }
  }

  /** Waits for the saved list; retries once if it failed. False if it is still unreadable. */
  async function ensureLoaded(): Promise<boolean> {
    await loading;
    if (!loaded) await get().load();
    return loaded;
  }

  return {
    items: [],
    transitions: [],

    load: () => {
      const ids = useSettings.getState().settings?.setList ?? [];
      loaded = ids.length === 0;
      loading = loaded
        ? Promise.resolve()
        : reload(ids).catch((error: unknown) => {
            notifyError(error, i18n.t("errors.setlist.load"));
          });
      return loading;
    },

    refresh: async () => {
      if (!(await ensureLoaded())) return;
      const ids = get().items.map((t) => t.id);
      if (ids.length === 0) return;
      try {
        await reload(ids);
      } catch (error) {
        notifyError(error, i18n.t("errors.setlist.refresh"));
      }
    },

    add: async (ids, index) => {
      if (!(await ensureLoaded())) return;
      const current = get().items;
      const known = new Set(current.map((t) => t.id));
      const fresh = ids.filter((id) => !known.has(id));
      if (fresh.length === 0) return;
      try {
        const rows = await rowsFor(fresh);
        const items = [...get().items];
        const at =
          index === undefined ? items.length : Math.max(0, Math.min(items.length, index));
        items.splice(at, 0, ...rows);
        await commit(items);
      } catch (error) {
        notifyError(error, i18n.t("errors.setlist.add"));
      }
    },

    move: async (from, to) => {
      if (!(await ensureLoaded())) return;
      const items = [...get().items];
      if (from === to || from < 0 || from >= items.length) return;
      const [moved] = items.splice(from, 1);
      if (!moved) return;
      items.splice(Math.max(0, Math.min(items.length, to)), 0, moved);
      await commit(items);
    },

    remove: async (id) => {
      if (!(await ensureLoaded())) return;
      await commit(get().items.filter((t) => t.id !== id));
    },

    clear: async () => {
      if (!(await ensureLoaded())) return;
      await commit([]);
    },

    saveAsCrate: async (name) => {
      const ids = get().items.map((t) => t.id);
      if (!name.trim() || ids.length === 0) return false;
      try {
        // One call: Rust creates the crate and adds the set in one step, so
        // a failure never leaves an empty crate behind.
        await api().crateCreate(name.trim(), "lime", ids);
        return true;
      } catch (error) {
        notifyError(error, i18n.t("errors.setlist.saveCrate"));
        return false;
      }
    },
  };
});

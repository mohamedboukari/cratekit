import { useTable } from "@tanstack/react-table";
import {
  defaultRangeExtractor,
  useVirtualizer,
  type Range,
} from "@tanstack/react-virtual";
import { cn } from "cn";
import { ArrowDown, ArrowUp, FolderInput, Search } from "lucide-react";
import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  type JSX,
  type KeyboardEvent,
  type MouseEvent,
  type PointerEvent,
} from "react";
import { useTranslation } from "react-i18next";

import { browserColumns, browserFeatures } from "@/components/browser/columns";
import { TrackContextMenu } from "@/components/browser/TrackContextMenu";
import { Button } from "@/components/ui/button";
import { pressToDrag } from "@/lib/drag";
import { formatDuration, formatTime } from "@/lib/format";
import { audioFormatsText } from "@/lib/formats";
import { api } from "@/lib/ipc";
import { announceImport, notifyError } from "@/lib/notify";
import type { browser } from "@/i18n/en/browser";
import type { TrackId, TrackRow } from "@/lib/types";
import { activeJobs, useJobs } from "@/stores/jobs";
import { useLibrary } from "@/stores/library";
import { usePlayer } from "@/stores/player";

const ROW_HEIGHT = 30;
/**
 * Column layout, by the width of the table area (container query on the
 * scroller): narrow shows #, title, artist, BPM, key, time; from 36rem it adds
 * energy and the ready tags.
 */
const GRID_COLUMNS =
  "grid grid-cols-[32px_minmax(88px,2fr)_minmax(56px,1.3fr)_56px_48px_56px] @xl:grid-cols-[36px_minmax(110px,2fr)_minmax(80px,1.3fr)_56px_48px_76px_56px_120px]";
/** Cells that only fit in the wide layout. */
const WIDE_ONLY = "hidden @xl:block";
/** Header text of each sortable column, as a key under `browser.columns`. */
const SORTABLE_HEADERS: Record<string, keyof typeof browser.columns> = {
  title: "title",
  artist: "artist",
  bpm: "bpm",
  camelot: "key",
  energy: "energy",
  durationSec: "time",
};

/**
 * Track browser: virtualized, sortable table of the current crate or library
 * (a real <table role="grid">, rows positioned by the virtualizer). One tab
 * stop; arrow keys move the active row (aria-activedescendant), Shift extends
 * the selection, Enter loads the track, Cmd/Ctrl+A selects all. Filters and
 * search are applied by Rust (setQuery); sorting is presentation only.
 */
export function TrackBrowser(): JSX.Element {
  const { t } = useTranslation();
  const rows = useLibrary((s) => s.rows);
  const status = useLibrary((s) => s.status);
  const query = useLibrary((s) => s.query);
  const crates = useLibrary((s) => s.crates);
  const counts = useLibrary((s) => s.counts);
  const jobs = useJobs((s) => s.jobs);
  // Live job progress per row, from the tracks each running job reports.
  const busy = useMemo(() => {
    const map = new Map<TrackId, string>();
    for (const job of activeJobs(jobs)) {
      const pct = job.total > 0 ? Math.round((job.done / job.total) * 100) : 0;
      const label = t("browser.tags.busy", {
        kind: t(`browser.tags.jobs.${job.kind}`),
        percent: pct,
      });
      for (const id of job.trackIds) map.set(id, label);
    }
    return map;
  }, [jobs, t]);
  const selectedIds = useLibrary((s) => s.selectedIds);
  const select = useLibrary((s) => s.select);
  const setQuery = useLibrary((s) => s.setQuery);
  const detail = usePlayer((s) => s.detail);
  const load = usePlayer((s) => s.load);

  const onSearch = useCallback((search: string) => void setQuery({ search }), [setQuery]);
  // First click sorts ascending for every column (lowest BPM, A to Z) as DJs expect.
  const table = useTable({
    features: browserFeatures,
    columns: browserColumns,
    data: rows,
    sortDescFirst: false,
  });
  const sortedRows = table.getRowModel().rows;
  const headers = table.getHeaderGroups()[0]?.headers ?? [];

  // Active row and range anchor are track ids, so they stay on the same track after a sort or filter.
  const [activeId, setActiveId] = useState<TrackId | null>(null);
  const [anchorId, setAnchorId] = useState<TrackId | null>(null);
  const [menuTrackId, setMenuTrackId] = useState<TrackId | null>(null);
  const indexById = useMemo(
    () => new Map(sortedRows.map((r, i) => [r.original.id, i])),
    [sortedRows],
  );
  const activeIndex = activeId === null ? -1 : (indexById.get(activeId) ?? -1);
  const anchorIndex = anchorId === null ? -1 : (indexById.get(anchorId) ?? -1);

  // The active row always stays rendered, so aria-activedescendant points at a real element.
  const rangeExtractor = useCallback(
    (range: Range): number[] => {
      const indexes = defaultRangeExtractor(range);
      if (activeIndex < 0 || indexes.includes(activeIndex)) return indexes;
      return [...indexes, activeIndex].toSorted((a, b) => a - b);
    },
    [activeIndex],
  );
  const scrollRef = useRef<HTMLDivElement>(null);
  const virtualizer = useVirtualizer({
    count: sortedRows.length,
    getScrollElement: () => scrollRef.current,
    estimateSize: () => ROW_HEIGHT,
    getItemKey: (index) => sortedRows[index]?.original.id ?? index,
    overscan: 12,
    rangeExtractor,
    initialRect: { width: 900, height: 480 },
  });
  const selected = useMemo(() => new Set(selectedIds), [selectedIds]);
  const matching = useMemo(() => {
    const analysis = detail?.analysis;
    return analysis ? new Set([analysis.camelot, ...analysis.mixesWellWith]) : null;
  }, [detail]);

  const idAt = (index: number): TrackId | undefined => sortedRows[index]?.original.id;

  const selectRange = (from: number, to: number): void => {
    const [lo, hi] = from < to ? [from, to] : [to, from];
    const ids: TrackId[] = [];
    for (let i = lo; i <= hi; i++) {
      const id = idAt(i);
      if (id !== undefined) ids.push(id);
    }
    select(ids);
  };

  const moveActive = (index: number, extend: boolean): void => {
    if (sortedRows.length === 0) return;
    const next = Math.max(0, Math.min(sortedRows.length - 1, index));
    const id = idAt(next);
    if (id === undefined) return;
    setActiveId(id);
    virtualizer.scrollToIndex(next, { align: "auto" });
    if (extend && anchorIndex >= 0) selectRange(anchorIndex, next);
    else {
      setAnchorId(id);
      select([id]);
    }
  };

  const onKeyDown = (event: KeyboardEvent<HTMLTableElement>): void => {
    const page = Math.max(
      1,
      Math.floor((scrollRef.current?.clientHeight ?? 300) / ROW_HEIGHT) - 1,
    );
    const mod = event.metaKey || event.ctrlKey;
    let handled = true;
    if (event.key === "ArrowDown") moveActive(activeIndex + 1, event.shiftKey);
    else if (event.key === "ArrowUp") moveActive(activeIndex - 1, event.shiftKey);
    else if (event.key === "PageDown") moveActive(activeIndex + page, event.shiftKey);
    else if (event.key === "PageUp") moveActive(activeIndex - page, event.shiftKey);
    else if (event.key === "Home") moveActive(0, event.shiftKey);
    else if (event.key === "End") moveActive(sortedRows.length - 1, event.shiftKey);
    else if (event.key === "Enter") {
      const id = idAt(activeIndex);
      if (id !== undefined) void load(id);
    } else if (mod && event.key.toLowerCase() === "a")
      select(sortedRows.map((r) => r.original.id));
    else handled = false;
    if (handled) event.preventDefault();
  };

  const onRowClick =
    (index: number, id: TrackId) => (event: MouseEvent<HTMLTableRowElement>) => {
      setActiveId(id);
      if (event.shiftKey && anchorIndex >= 0) {
        selectRange(anchorIndex, index);
        return;
      }
      setAnchorId(id);
      if (event.metaKey || event.ctrlKey) {
        select(
          selected.has(id) ? selectedIds.filter((x) => x !== id) : [...selectedIds, id],
        );
      } else select([id]);
    };

  /**
   * Works out which track the context menu is for: the row under the pointer,
   * the active row when opened from the keyboard (Shift+F10 on the grid), or
   * none (header, empty space), which disables the track actions.
   */
  const onContextMenu = (event: MouseEvent<HTMLDivElement>): void => {
    const target = event.target instanceof Element ? event.target : null;
    const row = target?.closest<HTMLElement>("tr[data-track-id]");
    const fromKeyboard = target?.getAttribute("role") === "grid";
    const id = row ? Number(row.dataset.trackId) : fromKeyboard ? activeId : null;
    if (id === null || !Number.isInteger(id)) {
      setMenuTrackId(null);
      return;
    }
    setMenuTrackId(id);
    setActiveId(id);
    if (!selected.has(id)) {
      setAnchorId(id);
      select([id]);
    }
  };

  // A drag carries the selection when it starts on a selected row, else that row.
  const onRowPointerDown =
    (id: TrackId, title: string) => (event: PointerEvent<HTMLTableRowElement>) =>
      pressToDrag(event, () => {
        const ids = selected.has(id) ? selectedIds : [id];
        const label =
          ids.length === 1 ? title : t("browser.dragTracks", { count: ids.length });
        return { kind: "tracks", ids, label };
      });

  const crateName =
    query.crateId === null
      ? t("browser.allTracks")
      : (crates.find((c) => c.id === query.crateId)?.name ?? t("browser.crate"));
  const totalSec = rows.reduce((sum, r) => sum + r.durationSec, 0);
  const deckBpm = detail?.track.bpm ?? null;
  const deckCamelot = detail?.track.camelot ?? null;

  return (
    <section aria-label={t("browser.label")} className="flex h-full min-h-0 flex-col">
      <header className="flex flex-wrap items-center gap-2.5 border-b border-border px-4 py-2">
        <h2 className="font-semibold">{crateName}</h2>
        <span className="text-muted-foreground">
          {status === "loading"
            ? t("browser.loading")
            : t("browser.summary", {
                count: rows.length,
                duration: formatDuration(totalSec),
              })}
        </span>
        <SearchBox value={query.search} onChange={onSearch} />
        <div className="ml-auto flex flex-wrap gap-1.5">
          {/* A pressed chip names the value it filters by, even after another track loads. */}
          <FilterChip
            label={
              query.matchCamelot !== null
                ? t("browser.filters.matchesKey", { key: query.matchCamelot })
                : deckCamelot
                  ? t("browser.filters.matchesKey", { key: deckCamelot })
                  : t("browser.filters.matchesDeckKey")
            }
            pressed={query.matchCamelot !== null}
            disabled={query.matchCamelot === null && !deckCamelot}
            onToggle={() =>
              void setQuery({
                matchCamelot: query.matchCamelot !== null ? null : deckCamelot,
              })
            }
          />
          {/* Rust applies the mixing range around nearBpm. */}
          <FilterChip
            label={
              query.nearBpm !== null
                ? t("browser.filters.nearBpm", { bpm: Math.round(query.nearBpm) })
                : deckBpm
                  ? t("browser.filters.nearBpm", { bpm: Math.round(deckBpm) })
                  : t("browser.filters.nearDeckBpm")
            }
            pressed={query.nearBpm !== null}
            disabled={query.nearBpm === null && !deckBpm}
            onToggle={() =>
              void setQuery({ nearBpm: query.nearBpm !== null ? null : deckBpm })
            }
          />
          <FilterChip
            label={t("browser.filters.hasStems")}
            pressed={query.hasStems}
            onToggle={() => void setQuery({ hasStems: !query.hasStems })}
          />
        </div>
      </header>

      {counts?.all === 0 && status === "ready" ? (
        <EmptyLibrary />
      ) : rows.length === 0 && status !== "loading" ? (
        <div className="grid flex-1 place-items-center text-muted-foreground">
          {t("browser.noMatch")}
        </div>
      ) : (
        <TrackContextMenu trackId={menuTrackId} selectedIds={selectedIds}>
          <div
            ref={scrollRef}
            onContextMenu={onContextMenu}
            className="@container min-h-0 flex-1 overflow-y-auto px-2"
          >
            <table
              // WAI-ARIA data grid pattern: <table role="grid"> with aria-activedescendant.
              // oxlint-disable-next-line jsx-a11y/no-noninteractive-element-to-interactive-role
              role="grid"
              tabIndex={0}
              aria-label={t("browser.table.label", {
                crate: crateName,
                count: rows.length,
              })}
              aria-rowcount={sortedRows.length + 1}
              aria-multiselectable="true"
              aria-activedescendant={
                activeIndex >= 0 ? `track-row-${activeId}` : undefined
              }
              onKeyDown={onKeyDown}
              className="grid w-full -outline-offset-2"
            >
              <thead className="sticky top-0 z-10 grid bg-background">
                <tr
                  aria-rowindex={1}
                  className={cn(
                    GRID_COLUMNS,
                    "border-b border-border text-[11px] tracking-wider text-muted-foreground uppercase",
                  )}
                >
                  <th scope="col" className="px-2 py-1.5 text-left font-normal">
                    #
                  </th>
                  {headers.map((header) => {
                    const sorted = header.column.getIsSorted();
                    const headerKey = SORTABLE_HEADERS[header.id];
                    return (
                      <th
                        key={header.id}
                        scope="col"
                        aria-sort={
                          sorted === "asc"
                            ? "ascending"
                            : sorted === "desc"
                              ? "descending"
                              : "none"
                        }
                        className={cn(
                          "min-w-0 font-normal",
                          header.id === "energy" && WIDE_ONLY,
                        )}
                      >
                        <button
                          type="button"
                          onClick={header.column.getToggleSortingHandler()}
                          className="flex w-full items-center gap-1 px-2 py-1.5 text-left uppercase hover:text-foreground"
                        >
                          {headerKey ? t(`browser.columns.${headerKey}`) : header.id}
                          {sorted === "asc" && (
                            <ArrowUp aria-hidden="true" className="size-3" />
                          )}
                          {sorted === "desc" && (
                            <ArrowDown aria-hidden="true" className="size-3" />
                          )}
                        </button>
                      </th>
                    );
                  })}
                  <th
                    scope="col"
                    className={cn("px-2 py-1.5 text-left font-normal", WIDE_ONLY)}
                  >
                    {t("browser.table.ready")}
                  </th>
                </tr>
              </thead>
              <tbody
                className="relative grid"
                style={{ height: virtualizer.getTotalSize() }}
              >
                {virtualizer.getVirtualItems().map((item) => {
                  const row = sortedRows[item.index];
                  if (!row) return null;
                  const track = row.original;
                  return (
                    <TrackRowView
                      key={track.id}
                      track={track}
                      index={item.index}
                      top={item.start}
                      selected={selected.has(track.id)}
                      active={item.index === activeIndex}
                      loaded={detail?.track.id === track.id}
                      busy={busy.get(track.id) ?? null}
                      keyMatch={
                        matching !== null &&
                        track.camelot !== null &&
                        matching.has(track.camelot)
                      }
                      onClick={onRowClick(item.index, track.id)}
                      onDoubleClick={() => void load(track.id)}
                      onPointerDown={onRowPointerDown(track.id, track.title)}
                    />
                  );
                })}
              </tbody>
            </table>
          </div>
        </TrackContextMenu>
      )}
    </section>
  );
}

interface TrackRowViewProps {
  track: TrackRow;
  index: number;
  top: number;
  selected: boolean;
  active: boolean;
  loaded: boolean;
  /** A running job on this track, e.g. "analysis 40%". */
  busy: string | null;
  keyMatch: boolean;
  onClick: (event: MouseEvent<HTMLTableRowElement>) => void;
  onDoubleClick: () => void;
  onPointerDown: (event: PointerEvent<HTMLTableRowElement>) => void;
}

function TrackRowView({
  track,
  index,
  top,
  selected,
  active,
  loaded,
  busy,
  keyMatch,
  onClick,
  onDoubleClick,
  onPointerDown,
}: TrackRowViewProps): JSX.Element {
  const { t } = useTranslation();
  return (
    // Keyboard access is on the grid (aria-activedescendant), not on each row.
    // oxlint-disable-next-line jsx-a11y/click-events-have-key-events
    <tr
      id={`track-row-${track.id}`}
      data-track-id={track.id}
      aria-rowindex={index + 2}
      aria-selected={selected}
      onClick={onClick}
      onDoubleClick={onDoubleClick}
      onPointerDown={onPointerDown}
      style={{ transform: `translateY(${top}px)`, height: ROW_HEIGHT }}
      className={cn(
        GRID_COLUMNS,
        "absolute inset-x-0 items-center rounded-sm text-[13px]",
        selected ? "bg-secondary" : "hover:bg-card",
        loaded && "bg-primary/10 shadow-[inset_2px_0_0_var(--color-primary)]",
        active && "outline-1 -outline-offset-1 outline-ring/60",
      )}
    >
      <td className="px-2 font-mono text-[11px] text-muted-foreground tabular-nums">
        {index + 1}
      </td>
      <td className="truncate px-2" title={track.title}>
        {track.title}
      </td>
      <td className="truncate px-2 text-muted-foreground" title={track.artist}>
        {track.artist}
      </td>
      <td className="px-2 font-mono tabular-nums">{track.bpm?.toFixed(1) ?? "--"}</td>
      <td
        className={cn(
          "px-2 font-mono font-semibold tabular-nums",
          keyMatch && "text-primary",
        )}
      >
        {track.camelot ?? "--"}
      </td>
      <td
        className={cn("px-2", WIDE_ONLY)}
        aria-label={
          track.energy
            ? t("browser.table.energy", { value: track.energy })
            : t("browser.table.energyUnknown")
        }
      >
        <EnergyBars value={track.energy} />
      </td>
      <td className="px-2 font-mono text-muted-foreground tabular-nums">
        {formatTime(track.durationSec)}
      </td>
      <td className="hidden gap-1 overflow-hidden px-2 @xl:flex">
        {busy ? (
          <Tag accent>{busy}</Tag>
        ) : track.activity ? (
          <Tag accent>
            {t("browser.tags.activity", {
              kind: t(`browser.tags.jobs.${track.activity.kind}`),
            })}
          </Tag>
        ) : track.analyzed ? (
          <Tag accent>{t("browser.tags.analyzed")}</Tag>
        ) : (
          <Tag>{t("browser.tags.notAnalyzed")}</Tag>
        )}
        {track.hasStems && <Tag accent>{t("browser.tags.stems")}</Tag>}
        {track.mastered && <Tag accent>{t("browser.tags.mastered")}</Tag>}
      </td>
    </tr>
  );
}

function EnergyBars({ value }: { value: number | null }): JSX.Element {
  return (
    <span aria-hidden="true" className="inline-flex gap-0.5">
      {Array.from({ length: 10 }, (_, i) => (
        <i
          key={i}
          className={cn(
            "h-2.5 w-1 rounded-[1px]",
            value !== null && i < value ? "bg-ck-blue" : "bg-secondary",
          )}
        />
      ))}
    </span>
  );
}

function Tag({ children, accent }: { children: string; accent?: boolean }): JSX.Element {
  return (
    <span
      className={cn(
        "rounded-sm bg-secondary px-1.5 text-[11px]",
        accent ? "text-primary" : "text-muted-foreground",
      )}
    >
      {children}
    </span>
  );
}

function FilterChip({
  label,
  pressed,
  disabled,
  onToggle,
}: {
  label: string;
  pressed: boolean;
  disabled?: boolean;
  onToggle: () => void;
}): JSX.Element {
  return (
    <Button
      variant="outline"
      size="sm"
      aria-pressed={pressed}
      disabled={disabled}
      onClick={onToggle}
      className={cn("rounded-full", pressed && "border-primary text-primary")}
    >
      {label}
    </Button>
  );
}

/** Search box: updates the query after a short pause in typing. */
function SearchBox({
  value,
  onChange,
}: {
  value: string;
  onChange: (value: string) => void;
}): JSX.Element {
  const { t } = useTranslation();
  const [text, setText] = useState(value);
  useEffect(() => {
    if (text === value) return;
    const timer = setTimeout(() => onChange(text), 180);
    return () => clearTimeout(timer);
  }, [text, value, onChange]);
  return (
    <label className="relative flex items-center">
      <span className="sr-only">{t("browser.search.label")}</span>
      <Search
        aria-hidden="true"
        className="pointer-events-none absolute left-2 size-3.5 text-muted-foreground"
      />
      <input
        type="search"
        value={text}
        onChange={(e) => setText(e.target.value)}
        placeholder={t("browser.search.placeholder")}
        className="h-7 w-48 rounded-md border border-border bg-secondary pl-7 text-[13px] placeholder:text-muted-foreground"
      />
    </label>
  );
}

/** First run: nothing in the library yet. Drop anywhere on the window, or pick files. */
function EmptyLibrary(): JSX.Element {
  const { t, i18n } = useTranslation();
  const addPaths = useLibrary((s) => s.addPaths);
  const [adding, setAdding] = useState(false);
  const add = async (pick: () => Promise<string[]>): Promise<void> => {
    try {
      const paths = await pick();
      if (paths.length === 0) return;
      setAdding(true);
      announceImport(await addPaths(paths));
    } catch (error) {
      notifyError(error, t("browser.errors.filePicker"));
    } finally {
      setAdding(false);
    }
  };
  return (
    <div className="flex flex-1 p-4">
      <div className="flex flex-1 flex-col items-center justify-center gap-3 rounded-xl border-2 border-dashed border-border text-center">
        <FolderInput aria-hidden="true" className="size-9 text-muted-foreground" />
        <p className="text-base font-semibold">{t("browser.empty.title")}</p>
        <p className="max-w-80 text-muted-foreground">
          {t("browser.empty.hint", { formats: audioFormatsText(i18n.language) })}
        </p>
        <div className="flex gap-2">
          <Button
            disabled={adding}
            onClick={() => void add(() => api().dialogOpenFiles())}
          >
            {adding ? t("browser.empty.adding") : t("browser.empty.addTracks")}
          </Button>
          <Button
            variant="outline"
            disabled={adding}
            onClick={() =>
              void add(async () => {
                const folder = await api().dialogOpenFolder();
                return folder ? [folder] : [];
              })
            }
          >
            {t("browser.empty.addFolder")}
          </Button>
        </div>
      </div>
    </div>
  );
}

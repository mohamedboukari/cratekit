import { cn } from "cn";
import { AlertTriangle, ListMusic, X } from "lucide-react";
import {
  useEffect,
  useMemo,
  useRef,
  useState,
  type DragEvent,
  type FormEvent,
  type JSX,
  type KeyboardEvent,
  type MouseEvent,
} from "react";
import { toast } from "sonner";

import { Button } from "@/components/ui/button";
import { parseTrackDrop, TRACK_DRAG_MIME } from "@/lib/dnd";
import { releasePointerFocus } from "@/lib/focus";
import { formatDuration } from "@/lib/format";
import type { TrackId, TrackRow, Transition } from "@/lib/types";
import { useLibrary } from "@/stores/library";
import { usePlayer } from "@/stores/player";
import { useSetList } from "@/stores/setlist";

/** Reordering drags inside the list use their own type, so they never look like a browser drop. */
const REORDER_MIME = "application/x-cratekit-setlist-index";

function acceptsDrop(event: DragEvent<HTMLElement>): boolean {
  const types = event.dataTransfer.types;
  return types.includes(TRACK_DRAG_MIME) || types.includes(REORDER_MIME);
}

/**
 * Set builder side list (#55): stage tracks and build a set's running order.
 * Each item is a draggable button: drop tracks from the browser, reorder by
 * drag or Alt+Arrow keys, Delete to remove, Enter to load on the deck.
 * Key and BPM fit between neighbours come from Rust.
 */
export function SetList(): JSX.Element {
  const items = useSetList((s) => s.items);
  const transitions = useSetList((s) => s.transitions);
  const add = useSetList((s) => s.add);
  const move = useSetList((s) => s.move);
  const remove = useSetList((s) => s.remove);
  const clear = useSetList((s) => s.clear);
  const saveAsCrate = useSetList((s) => s.saveAsCrate);
  const refreshRows = useLibrary((s) => s.refreshRows);
  const load = usePlayer((s) => s.load);
  const [dropIndex, setDropIndex] = useState<number | null>(null);
  const [naming, setNaming] = useState(false);
  const [name, setName] = useState("");
  const nameInput = useRef<HTMLInputElement>(null);
  const saveButton = useRef<HTMLButtonElement>(null);
  const heading = useRef<HTMLHeadingElement>(null);
  const itemButtons = useRef(new Map<TrackId, HTMLButtonElement>());
  const wasNaming = useRef(false);

  useEffect(() => {
    if (naming) nameInput.current?.focus();
    else if (wasNaming.current) saveButton.current?.focus();
    wasNaming.current = naming;
  }, [naming]);

  // A drag that ends anywhere (dropped elsewhere, Escape) clears the drop line.
  useEffect(() => {
    const reset = (): void => setDropIndex(null);
    window.addEventListener("dragend", reset);
    window.addEventListener("drop", reset);
    return () => {
      window.removeEventListener("dragend", reset);
      window.removeEventListener("drop", reset);
    };
  }, []);

  // Warnings are looked up by the neighbour pair, so a stale answer never lands on the wrong pair.
  const transitionByPair = useMemo(
    () => new Map(transitions.map((t) => [`${t.fromId}:${t.toId}`, t])),
    [transitions],
  );
  const totalSec = items.reduce((sum, t) => sum + t.durationSec, 0);

  /** After a keyboard move or remove, keeps focus in the list (React re-inserts moved nodes). */
  const focusAfter = async (
    action: Promise<void>,
    target: TrackId | "heading",
  ): Promise<void> => {
    await action;
    if (target === "heading") heading.current?.focus();
    else itemButtons.current.get(target)?.focus();
  };

  const onListDragLeave = (event: DragEvent<HTMLElement>): void => {
    const next = event.relatedTarget;
    if (!(next instanceof Node) || !event.currentTarget.contains(next))
      setDropIndex(null);
  };

  const onDragOver = (index: number) => (event: DragEvent<HTMLElement>) => {
    if (!acceptsDrop(event)) return;
    event.preventDefault();
    event.dataTransfer.dropEffect = event.dataTransfer.types.includes(REORDER_MIME)
      ? "move"
      : "copy";
    setDropIndex(index);
  };

  const onDrop = (index: number) => (event: DragEvent<HTMLElement>) => {
    event.preventDefault();
    setDropIndex(null);
    const reorder = event.dataTransfer.getData(REORDER_MIME);
    if (reorder !== "") {
      const from = Number(reorder);
      if (Number.isInteger(from)) void move(from, from < index ? index - 1 : index);
      return;
    }
    void add(parseTrackDrop(event.dataTransfer.getData(TRACK_DRAG_MIME)), index);
  };

  const onItemKeyDown =
    (index: number, track: TrackRow) => (event: KeyboardEvent<HTMLButtonElement>) => {
      if (event.altKey && (event.key === "ArrowUp" || event.key === "ArrowDown")) {
        const to = event.key === "ArrowUp" ? index - 1 : index + 1;
        if (to >= 0 && to < items.length) void focusAfter(move(index, to), track.id);
      } else if (event.key === "Delete" || event.key === "Backspace") {
        const next = items[index + 1]?.id ?? items[index - 1]?.id ?? "heading";
        void focusAfter(remove(track.id), next);
      } else return;
      event.preventDefault();
    };

  const onSave = async (event: FormEvent<HTMLFormElement>): Promise<void> => {
    event.preventDefault();
    if (await saveAsCrate(name)) {
      toast.success(`Saved "${name.trim()}" as a crate`);
      setNaming(false);
      setName("");
      await refreshRows();
    }
  };

  return (
    <aside
      aria-label="Set list"
      className="flex h-full min-h-0 flex-col border-l border-border"
    >
      <header className="flex items-center gap-2 border-b border-border px-3 py-2">
        <ListMusic aria-hidden="true" className="size-4 text-muted-foreground" />
        <h2 ref={heading} tabIndex={-1} className="font-semibold">
          Set list
        </h2>
        <span className="text-muted-foreground">
          {items.length} · {formatDuration(totalSec)}
        </span>
        <div className="ml-auto flex gap-1">
          <Button
            ref={saveButton}
            variant="ghost"
            size="sm"
            disabled={items.length === 0}
            onClick={() => setNaming(true)}
          >
            Save as crate
          </Button>
          <Button
            variant="ghost"
            size="sm"
            disabled={items.length === 0}
            onClick={() => void clear()}
          >
            Clear
          </Button>
        </div>
      </header>

      {naming && (
        <form
          onSubmit={(e) => void onSave(e)}
          className="flex gap-2 border-b border-border px-3 py-2"
        >
          <label className="sr-only" htmlFor="setlist-crate-name">
            Crate name
          </label>
          <input
            id="setlist-crate-name"
            ref={nameInput}
            value={name}
            onChange={(e) => setName(e.target.value)}
            onKeyDown={(e) => {
              if (e.key !== "Escape") return;
              e.stopPropagation();
              setNaming(false);
            }}
            placeholder="Crate name"
            className="h-7 min-w-0 flex-1 rounded-md border border-border bg-secondary px-2 text-[13px]"
          />
          <Button size="sm" type="submit" disabled={!name.trim()}>
            Save
          </Button>
        </form>
      )}

      {items.length === 0 ? (
        <div
          onDragOver={onDragOver(0)}
          onDragLeave={() => setDropIndex(null)}
          onDrop={onDrop(0)}
          className={cn(
            "m-3 grid flex-1 place-items-center rounded-lg border border-dashed border-border p-4 text-center text-muted-foreground",
            dropIndex !== null && "border-primary text-primary",
          )}
        >
          Drag tracks here to build your set, or right-click a track and choose Add to set
          list
        </div>
      ) : (
        <div onDragLeave={onListDragLeave} className="min-h-0 flex-1 overflow-y-auto p-2">
          <ol aria-label="Set order">
            {items.map((track, index) => (
              <SetListItem
                key={track.id}
                track={track}
                index={index}
                transition={
                  index > 0
                    ? transitionByPair.get(`${items[index - 1]?.id}:${track.id}`)
                    : undefined
                }
                dropBefore={dropIndex === index}
                onDragOver={onDragOver(index)}
                onDragEnd={() => setDropIndex(null)}
                onDrop={onDrop(index)}
                onKeyDown={onItemKeyDown(index, track)}
                buttonRef={(el) => {
                  if (el) itemButtons.current.set(track.id, el);
                  else itemButtons.current.delete(track.id);
                }}
                onRemove={() => void remove(track.id)}
                onLoad={(event) => {
                  releasePointerFocus(event);
                  void load(track.id);
                }}
              />
            ))}
          </ol>
          <div
            onDragOver={onDragOver(items.length)}
            onDragLeave={() => setDropIndex(null)}
            onDrop={onDrop(items.length)}
            className={cn(
              "mt-1 rounded-md border border-dashed border-transparent py-2 text-center text-[11px] text-muted-foreground",
              dropIndex === items.length && "border-primary text-primary",
            )}
          >
            Drop here to add at the end
          </div>
        </div>
      )}
    </aside>
  );
}

interface SetListItemProps {
  track: TrackRow;
  index: number;
  transition: Transition | undefined;
  dropBefore: boolean;
  onDragOver: (event: DragEvent<HTMLElement>) => void;
  onDragEnd: () => void;
  onDrop: (event: DragEvent<HTMLElement>) => void;
  onKeyDown: (event: KeyboardEvent<HTMLButtonElement>) => void;
  buttonRef: (el: HTMLButtonElement | null) => void;
  onRemove: () => void;
  onLoad: (event: MouseEvent<HTMLButtonElement>) => void;
}

function SetListItem({
  track,
  index,
  transition,
  dropBefore,
  onDragOver,
  onDragEnd,
  onDrop,
  onKeyDown,
  buttonRef,
  onRemove,
  onLoad,
}: SetListItemProps): JSX.Element {
  // Rust sends null when either key is unknown; only a known clash warns.
  const keyClash = transition?.keyCompatible === false;
  const bpmJump = transition?.bpmJump === true && transition.bpmChangePct !== null;
  const warnings = [
    keyClash ? "key clash with the previous track" : null,
    bpmJump && transition?.bpmChangePct != null
      ? `BPM ${transition.bpmChangePct > 0 ? "+" : ""}${transition.bpmChangePct}%`
      : null,
  ].filter((w): w is string => w !== null);

  return (
    <li
      className={cn(
        "group rounded-md",
        dropBefore && "shadow-[inset_0_2px_0_var(--color-primary)]",
      )}
    >
      {warnings.length > 0 && (
        <div className="flex items-center gap-1 px-2 pt-1 text-[11px] text-ck-orange">
          <AlertTriangle aria-hidden="true" className="size-3" />
          {warnings.join(" · ")}
        </div>
      )}
      <div className="flex items-center rounded-md hover:bg-card">
        <button
          ref={buttonRef}
          type="button"
          draggable
          onDragStart={(e) => {
            e.dataTransfer.setData(REORDER_MIME, String(index));
            e.dataTransfer.effectAllowed = "move";
          }}
          onDragOver={onDragOver}
          onDragEnd={onDragEnd}
          onDrop={onDrop}
          onKeyDown={onKeyDown}
          onClick={onLoad}
          aria-label={`${index + 1}. ${track.title}, load on deck${warnings.length ? `. ${warnings.join(", ")}` : ""}`}
          aria-keyshortcuts="Alt+ArrowUp Alt+ArrowDown Delete"
          className="flex min-w-0 flex-1 items-center gap-2 rounded-md px-2 py-1.5 text-left"
        >
          <span className="w-5 font-mono text-[11px] text-muted-foreground tabular-nums">
            {index + 1}
          </span>
          <span className="min-w-0 flex-1">
            <span className="block truncate" title={track.title}>
              {track.title}
            </span>
            <span className="block truncate text-[11px] text-muted-foreground">
              {track.artist}
            </span>
          </span>
          <span className="font-mono text-[11px] tabular-nums">
            {track.bpm?.toFixed(1) ?? "--"}
          </span>
          <span className="w-8 font-mono text-[11px] font-semibold text-primary tabular-nums">
            {track.camelot ?? "--"}
          </span>
        </button>
        <button
          type="button"
          onClick={onRemove}
          aria-label={`Remove ${track.title} from the set list`}
          className="mr-1 rounded-sm p-0.5 text-muted-foreground opacity-0 group-hover:opacity-100 hover:text-foreground focus-visible:opacity-100"
        >
          <X aria-hidden="true" className="size-3.5" />
        </button>
      </div>
    </li>
  );
}

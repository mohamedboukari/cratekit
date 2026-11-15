import type { JSX, ReactNode } from "react";

import {
  ContextMenu,
  ContextMenuContent,
  ContextMenuItem,
  ContextMenuSeparator,
  ContextMenuSub,
  ContextMenuSubContent,
  ContextMenuSubTrigger,
  ContextMenuTrigger,
} from "@/components/ui/context-menu";
import { api } from "@/lib/ipc";
import { notifyError } from "@/lib/notify";
import type { TrackId } from "@/lib/types";
import { useLibrary } from "@/stores/library";
import { useMaster } from "@/stores/master";
import { usePlayer } from "@/stores/player";
import { useSetList } from "@/stores/setlist";
import { useUi } from "@/stores/ui";

interface TrackContextMenuProps {
  /** The row that was right-clicked. */
  trackId: TrackId | null;
  /** Actions apply to the whole selection when it includes that row. */
  selectedIds: TrackId[];
  children: ReactNode;
}

/** Wraps a menu action: failures become an error toast. */
function run(action: () => Promise<unknown>, failure: string): () => void {
  return () => {
    action().catch((error: unknown) => notifyError(error, failure));
  };
}

/** One context menu for every row of the track browser. */
export function TrackContextMenu({
  trackId,
  selectedIds,
  children,
}: TrackContextMenuProps): JSX.Element {
  const crates = useLibrary((s) => s.crates);
  const refreshRows = useLibrary((s) => s.refreshRows);
  const load = usePlayer((s) => s.load);
  const addToSetList = useSetList((s) => s.add);
  const setReference = useMaster((s) => s.setReference);
  const openExport = useUi((s) => s.openExport);
  const rows = useLibrary((s) => s.rows);
  const targets =
    trackId !== null && selectedIds.includes(trackId)
      ? selectedIds
      : trackId !== null
        ? [trackId]
        : [];
  const plural = targets.length > 1 ? ` (${targets.length})` : "";

  return (
    <ContextMenu>
      <ContextMenuTrigger className="flex min-h-0 flex-1 flex-col">
        {children}
      </ContextMenuTrigger>
      <ContextMenuContent>
        <ContextMenuItem
          disabled={targets.length !== 1}
          onClick={() => trackId !== null && void load(trackId)}
        >
          Load on deck
        </ContextMenuItem>
        <ContextMenuItem
          disabled={targets.length === 0}
          onClick={() => void addToSetList(targets)}
        >
          Add to set list{plural}
        </ContextMenuItem>
        <ContextMenuSeparator />
        <ContextMenuItem
          disabled={targets.length === 0}
          onClick={run(
            () => api().analyzeTracks(targets),
            "Could not start the analysis",
          )}
        >
          Analyze{plural}
        </ContextMenuItem>
        <ContextMenuItem
          disabled={targets.length !== 1}
          onClick={run(async () => {
            const [id] = targets;
            if (id !== undefined) await api().stemsSplit(id);
          }, "Could not split stems")}
        >
          Split stems
        </ContextMenuItem>
        <ContextMenuItem
          disabled={targets.length !== 1}
          onClick={() => {
            const row = rows.find((r) => r.id === targets[0]);
            if (row) setReference({ id: row.id, title: row.title });
          }}
        >
          Use as mastering reference
        </ContextMenuItem>
        <ContextMenuItem
          disabled={targets.length === 0}
          onClick={() => openExport(targets)}
        >
          Export to VirtualDJ{plural}...
        </ContextMenuItem>
        <ContextMenuSub>
          <ContextMenuSubTrigger disabled={targets.length === 0}>
            Add to crate
          </ContextMenuSubTrigger>
          <ContextMenuSubContent>
            {crates.map((crate) => (
              <ContextMenuItem
                key={crate.id}
                onClick={run(async () => {
                  await api().crateAddTracks(crate.id, targets);
                  await refreshRows();
                }, `Could not add to ${crate.name}`)}
              >
                {crate.name}
              </ContextMenuItem>
            ))}
          </ContextMenuSubContent>
        </ContextMenuSub>
        <ContextMenuSeparator />
        <ContextMenuItem disabled title="Arrives with the library task">
          Show in folder
        </ContextMenuItem>
        <ContextMenuItem disabled title="Arrives with the library task">
          Remove from library
        </ContextMenuItem>
      </ContextMenuContent>
    </ContextMenu>
  );
}

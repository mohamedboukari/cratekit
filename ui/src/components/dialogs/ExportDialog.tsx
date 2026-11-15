import { useEffect, useState, type JSX } from "react";
import { toast } from "sonner";

import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogTitle,
} from "@/components/ui/dialog";
import { api } from "@/lib/ipc";
import { notifyError } from "@/lib/notify";
import type { ExportPlan, TrackId } from "@/lib/types";
import { useUi } from "@/stores/ui";

function tracks(count: number): string {
  return `${count} ${count === 1 ? "track" : "tracks"}`;
}

/**
 * Export to VirtualDJ (SPEC, Module 6): Rust first does a dry run and lists
 * every change (BPM, key, cues) plus where it backs up the database; only
 * Confirm writes. While it writes, the dialog cannot be dismissed.
 */
export function ExportDialog(): JSX.Element {
  const ids = useUi((s) => s.exportIds);
  const close = useUi((s) => s.closeExport);
  const [writing, setWriting] = useState(false);
  return (
    <Dialog open={ids !== null} onOpenChange={(next) => !next && !writing && close()}>
      <DialogContent className="sm:max-w-lg" showCloseButton={!writing}>
        {ids !== null && (
          <ExportBody
            key={ids.join(",")}
            ids={ids}
            writing={writing}
            setWriting={setWriting}
            onDone={close}
          />
        )}
      </DialogContent>
    </Dialog>
  );
}

function ExportBody({
  ids,
  writing,
  setWriting,
  onDone,
}: {
  ids: TrackId[];
  writing: boolean;
  setWriting: (writing: boolean) => void;
  onDone: () => void;
}): JSX.Element {
  const [plan, setPlan] = useState<ExportPlan | null>(null);
  const [failed, setFailed] = useState(false);

  useEffect(() => {
    let current = true;
    api()
      .exportVirtualdj(ids, true)
      .then((p) => current && setPlan(p))
      .catch((error: unknown) => {
        if (!current) return;
        setFailed(true);
        notifyError(error, "Could not prepare the VirtualDJ export");
      });
    return () => {
      current = false;
    };
  }, [ids]);

  const confirm = async (): Promise<void> => {
    setWriting(true);
    try {
      const done = await api().exportVirtualdj(ids, false);
      toast.success(`Exported ${tracks(done.tracks.length)} to VirtualDJ`, {
        description: `Backup: ${done.backupPath}`,
      });
      onDone();
    } catch (error) {
      notifyError(error, "The VirtualDJ export failed; your database was not changed");
    } finally {
      setWriting(false);
    }
  };

  const empty = plan !== null && plan.tracks.length === 0;

  return (
    <>
      <DialogTitle>Export to VirtualDJ</DialogTitle>
      <DialogDescription>
        {plan
          ? empty
            ? "Nothing to change: VirtualDJ already has this data."
            : `These changes will be written for ${tracks(plan.tracks.length)}. VirtualDJ's database is backed up first.`
          : failed
            ? "The export could not be prepared."
            : "Checking what would change..."}
      </DialogDescription>
      {plan && !empty && (
        <>
          <ul className="max-h-64 overflow-y-auto rounded-md border border-border">
            {plan.tracks.map((t) => (
              <li
                key={t.trackId}
                className="border-b border-border px-3 py-1.5 last:border-b-0"
              >
                <span className="block truncate font-medium" title={t.title}>
                  {t.title}
                </span>
                <span className="text-[11px] text-muted-foreground">
                  {t.changes.join(", ")}
                </span>
              </li>
            ))}
          </ul>
          <p className="text-[11px] text-muted-foreground">
            Backup:{" "}
            <span className="font-mono" title={plan.backupPath}>
              {plan.backupPath}
            </span>
          </p>
        </>
      )}
      <DialogFooter>
        <Button variant="outline" disabled={writing} onClick={onDone}>
          Cancel
        </Button>
        <Button disabled={!plan || empty || writing} onClick={() => void confirm()}>
          {writing ? "Writing..." : "Export"}
        </Button>
      </DialogFooter>
    </>
  );
}

import type { JSX } from "react";

import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogTitle,
} from "@/components/ui/dialog";
import type { Job } from "@/lib/types";

interface ModelDownloadDialogProps {
  open: boolean;
  sizeBytes: number | null;
  /** The running download, if any (its label says when Rust verifies the checksum). */
  job: Job | undefined;
  onStart: () => void;
  onCancelJob: (job: Job) => void;
  onClose: () => void;
}

/**
 * First-run download of the stems model: what it is and how big, then the
 * progress (download, then checksum) with cancel. The only network call
 * Cratekit makes (SPEC, Non-functional).
 */
export function ModelDownloadDialog({
  open,
  sizeBytes,
  job,
  onStart,
  onCancelJob,
  onClose,
}: ModelDownloadDialogProps): JSX.Element {
  const pct = job && job.total > 0 ? Math.round((job.done / job.total) * 100) : 0;
  const size =
    sizeBytes === null ? "about 200 MB" : `${Math.round(sizeBytes / 1_000_000)} MB`;
  return (
    <Dialog open={open} onOpenChange={(next) => !next && onClose()}>
      <DialogContent>
        <DialogTitle>Download the stems model</DialogTitle>
        <DialogDescription>
          {`Stem splitting uses the htdemucs model (${size}). It downloads once, its checksum is verified, and it stays on this computer. This is the only time Cratekit goes online.`}
        </DialogDescription>
        {job && (
          <div className="grid gap-1.5">
            <div className="flex justify-between">
              <span>{job.label}</span>
              <span className="font-mono tabular-nums">{pct}%</span>
            </div>
            <progress
              max={100}
              value={pct}
              aria-label={job.label}
              className="h-1.5 w-full appearance-none overflow-hidden rounded-full [&::-moz-progress-bar]:bg-primary [&::-webkit-progress-bar]:bg-secondary [&::-webkit-progress-value]:bg-primary"
            />
          </div>
        )}
        <DialogFooter>
          {job ? (
            <>
              <Button variant="outline" onClick={() => onCancelJob(job)}>
                Cancel download
              </Button>
              <Button variant="ghost" onClick={onClose}>
                Hide
              </Button>
            </>
          ) : (
            <>
              <Button variant="outline" onClick={onClose}>
                Not now
              </Button>
              <Button onClick={onStart}>{`Download ${size}`}</Button>
            </>
          )}
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

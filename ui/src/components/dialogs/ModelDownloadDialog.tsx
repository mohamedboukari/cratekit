import { jobLabel } from "@/lib/format";
import type { JSX } from "react";
import { useTranslation } from "react-i18next";

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
  const { t } = useTranslation();
  const pct = job && job.total > 0 ? Math.round((job.done / job.total) * 100) : 0;
  const size =
    sizeBytes === null
      ? t("dialogs.modelDownload.sizeUnknown")
      : t("dialogs.modelDownload.size", { mb: Math.round(sizeBytes / 1_000_000) });
  return (
    <Dialog open={open} onOpenChange={(next) => !next && onClose()}>
      <DialogContent>
        <DialogTitle>{t("dialogs.modelDownload.title")}</DialogTitle>
        <DialogDescription>
          {t("dialogs.modelDownload.description", { size })}
        </DialogDescription>
        {job && (
          <div className="grid gap-1.5">
            <div className="flex justify-between">
              <span>{jobLabel(job)}</span>
              <span className="font-mono tabular-nums">{pct}%</span>
            </div>
            <progress
              max={100}
              value={pct}
              aria-label={jobLabel(job)}
              className="h-1.5 w-full appearance-none overflow-hidden rounded-full [&::-moz-progress-bar]:bg-primary [&::-webkit-progress-bar]:bg-secondary [&::-webkit-progress-value]:bg-primary"
            />
          </div>
        )}
        <DialogFooter>
          {job ? (
            <>
              <Button variant="outline" onClick={() => onCancelJob(job)}>
                {t("dialogs.modelDownload.cancel")}
              </Button>
              <Button variant="ghost" onClick={onClose}>
                {t("dialogs.modelDownload.hide")}
              </Button>
            </>
          ) : (
            <>
              <Button variant="outline" onClick={onClose}>
                {t("dialogs.modelDownload.notNow")}
              </Button>
              <Button onClick={onStart}>
                {t("dialogs.modelDownload.download", { size })}
              </Button>
            </>
          )}
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

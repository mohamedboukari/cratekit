import { type TFunction } from "i18next";
import { useEffect, useState, type JSX } from "react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";

import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogTitle,
} from "@/components/ui/dialog";
import { i18n } from "@/i18n";
import { api } from "@/lib/ipc";
import { notifyError } from "@/lib/notify";
import type { ExportChangeItem, ExportPlan, TrackId } from "@/lib/types";
import { useUi } from "@/stores/ui";

/** One export change in the UI language (Rust sends the data, not the words). */
function changeText(change: ExportChangeItem, t: TFunction): string {
  const unknown = t("dialogs.export.changes.unknown");
  switch (change.field) {
    case "bpm":
      return t("dialogs.export.changes.bpm", {
        value: change.value?.toFixed(1) ?? unknown,
      });
    case "key":
      return t("dialogs.export.changes.key", { value: change.value ?? unknown });
    case "cues":
      return t("dialogs.export.changes.cues", { count: change.value });
  }
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
  const { t } = useTranslation();
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
        // Read at call time: a language switch must not re-run the dry run.
        notifyError(error, i18n.t("dialogs.errors.exportPrepare"));
      });
    return () => {
      current = false;
    };
  }, [ids]);

  const confirm = async (): Promise<void> => {
    setWriting(true);
    try {
      const done = await api().exportVirtualdj(ids, false);
      toast.success(t("dialogs.export.exported", { count: done.tracks.length }), {
        description: t("dialogs.export.backupPath", { path: done.backupPath }),
      });
      onDone();
    } catch (error) {
      notifyError(error, t("dialogs.errors.exportWrite"));
    } finally {
      setWriting(false);
    }
  };

  const empty = plan !== null && plan.tracks.length === 0;

  return (
    <>
      <DialogTitle>{t("dialogs.export.title")}</DialogTitle>
      <DialogDescription>
        {plan
          ? empty
            ? t("dialogs.export.nothingToChange")
            : t("dialogs.export.willWrite", { count: plan.tracks.length })
          : failed
            ? t("dialogs.export.prepareFailed")
            : t("dialogs.export.checking")}
      </DialogDescription>
      {plan && !empty && (
        <>
          <ul className="max-h-64 overflow-y-auto rounded-md border border-border">
            {plan.tracks.map((track) => (
              <li
                key={track.trackId}
                className="border-b border-border px-3 py-1.5 last:border-b-0"
              >
                <span className="block truncate font-medium" title={track.title}>
                  {track.title}
                </span>
                <span className="text-[11px] text-muted-foreground">
                  {track.changes.map((change) => changeText(change, t)).join(", ")}
                </span>
              </li>
            ))}
          </ul>
          <p className="text-[11px] text-muted-foreground">
            {t("dialogs.export.backup")}{" "}
            <span className="font-mono" title={plan.backupPath}>
              {plan.backupPath}
            </span>
          </p>
        </>
      )}
      <DialogFooter>
        <Button variant="outline" disabled={writing} onClick={onDone}>
          {t("dialogs.export.cancel")}
        </Button>
        <Button disabled={!plan || empty || writing} onClick={() => void confirm()}>
          {writing ? t("dialogs.export.writing") : t("dialogs.export.confirm")}
        </Button>
      </DialogFooter>
    </>
  );
}

import { cn } from "cn";
import { useState, type FormEvent, type JSX, type RefObject } from "react";
import { useTranslation } from "react-i18next";

import { CUE_BG } from "@/components/deck/cueColors";
import { Button } from "@/components/ui/button";
import { Dialog, DialogContent, DialogFooter, DialogTitle } from "@/components/ui/dialog";
import type { Cue, CueColor } from "@/lib/types";
import { usePlayer } from "@/stores/player";

const COLORS: readonly CueColor[] = [
  "orange",
  "blue",
  "pink",
  "purple",
  "lime",
  "yellow",
  "red",
  "teal",
];

/** Rename, recolour or delete a hot cue (right-click its flag, or the context menu key). */
export function CueEditDialog({
  cue,
  returnFocus,
  onClose,
}: {
  cue: Cue | null;
  /** Where focus goes on close (the flag itself is gone after Delete). */
  returnFocus?: RefObject<HTMLElement | null>;
  onClose: () => void;
}): JSX.Element {
  return (
    <Dialog open={cue !== null} onOpenChange={(next) => !next && onClose()}>
      <DialogContent finalFocus={returnFocus}>
        {cue && <CueForm key={cue.slot} cue={cue} onClose={onClose} />}
      </DialogContent>
    </Dialog>
  );
}

function CueForm({ cue, onClose }: { cue: Cue; onClose: () => void }): JSX.Element {
  const { t } = useTranslation();
  const updateHotCue = usePlayer((s) => s.updateHotCue);
  const clearHotCue = usePlayer((s) => s.clearHotCue);
  const [name, setName] = useState(cue.name ?? "");
  const [color, setColor] = useState<CueColor>(cue.color);

  const save = (event: FormEvent<HTMLFormElement>): void => {
    event.preventDefault();
    void updateHotCue(cue.slot, name.trim() || null, color);
    onClose();
  };

  return (
    <form onSubmit={save} className="grid gap-4">
      <DialogTitle>{t("deck.cueEditor.title", { slot: cue.slot })}</DialogTitle>
      <label className="grid gap-1">
        {t("deck.cueEditor.name")}
        <input
          value={name}
          onChange={(e) => setName(e.target.value)}
          placeholder={t("deck.cueEditor.namePlaceholder")}
          maxLength={40}
          className="h-7 rounded-md border border-border bg-secondary px-2 text-[13px]"
        />
      </label>
      <fieldset className="grid gap-1.5">
        <legend className="mb-1">{t("deck.cueEditor.colour")}</legend>
        <div className="flex flex-wrap gap-1.5">
          {COLORS.map((c) => (
            <label key={c} className="relative">
              <input
                type="radio"
                name={`cue-${cue.slot}-color`}
                value={c}
                checked={color === c}
                onChange={() => setColor(c)}
                className="peer sr-only"
              />
              <span
                className={cn(
                  "block size-7 rounded-md ring-offset-2 ring-offset-popover peer-checked:ring-2 peer-checked:ring-foreground peer-focus-visible:ring-2 peer-focus-visible:ring-ring",
                  CUE_BG[c],
                )}
              />
              <span className="sr-only">{t(`deck.cueEditor.colors.${c}`)}</span>
            </label>
          ))}
        </div>
      </fieldset>
      <DialogFooter className="sm:justify-between">
        <Button
          type="button"
          variant="destructive"
          onClick={() => {
            void clearHotCue(cue.slot);
            onClose();
          }}
        >
          {t("deck.cueEditor.delete")}
        </Button>
        <Button type="submit">{t("deck.cueEditor.save")}</Button>
      </DialogFooter>
    </form>
  );
}

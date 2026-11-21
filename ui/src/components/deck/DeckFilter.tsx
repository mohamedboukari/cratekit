import type { JSX } from "react";
import { useTranslation } from "react-i18next";

import { Knob } from "@/components/controls/Knob";
import { filterText } from "@/lib/format";
import { usePlayer } from "@/stores/player";

/** Size of the filter knob (px): a big target, like the DJ apps' mixer knobs. */
const FILTER_KNOB_PX = 80;

/**
 * The deck's DJ filter, in its own column beside the waveform (where v0.2's
 * channel strip goes): low-pass to the left, high-pass to the right, Off in
 * the middle with a detent.
 */
export function DeckFilter(): JSX.Element {
  const { t } = useTranslation();
  const filter = usePlayer((s) => s.filter);
  const setFilter = usePlayer((s) => s.setFilter);
  return (
    <div className="flex shrink-0 items-center border-l border-border pl-3">
      <Knob
        label={t("deck.filter.label")}
        value={filter}
        min={-1}
        max={1}
        defaultValue={0}
        bipolar
        detent
        startLabel={t("deck.filter.lowShort")}
        endLabel={t("deck.filter.highShort")}
        valueText={filterText(filter)}
        onChange={(v) => void setFilter(v)}
        size={FILTER_KNOB_PX}
      />
    </div>
  );
}

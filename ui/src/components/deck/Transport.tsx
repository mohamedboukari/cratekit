import { cn } from "cn";
import { Pause, Play } from "lucide-react";
import { useEffect, useRef, type JSX, type MouseEvent } from "react";
import { useTranslation } from "react-i18next";

import { CUE_BG } from "@/components/deck/cueColors";
import { Button } from "@/components/ui/button";
import { usePlaybackPositionRef } from "@/hooks/usePlaybackRefs";
import { releasePointerFocus } from "@/lib/focus";
import { formatTime } from "@/lib/format";
import type { CueSlot, TrackDetail } from "@/lib/types";
import { usePlayer } from "@/stores/player";
import { useUi } from "@/stores/ui";

const PAGE_SLOTS: Record<1 | 2, readonly CueSlot[]> = {
  1: [1, 2, 3, 4],
  2: [5, 6, 7, 8],
};

/** Transport row: CUE, play, hot cue pads (paged), time, what you hear (the filter is beside it, DeckFilter). */
export function Transport({ detail }: { detail: TrackDetail }): JSX.Element {
  const { t } = useTranslation();
  const playing = usePlayer((s) => s.playing);
  const mode = usePlayer((s) => s.mode);
  const togglePlay = usePlayer((s) => s.togglePlay);
  const cue = usePlayer((s) => s.cue);
  const triggerHotCue = usePlayer((s) => s.triggerHotCue);
  const clearHotCue = usePlayer((s) => s.clearHotCue);
  const padPage = useUi((s) => s.padPage);
  const togglePadPage = useUi((s) => s.togglePadPage);
  const position = usePlaybackPositionRef();
  const timeEl = useRef<HTMLSpanElement>(null);

  // Live time at 30 Hz without React re-renders.
  useEffect(() => {
    let frame = 0;
    let last = -1;
    const draw = (): void => {
      const second = Math.floor(position.current);
      if (second !== last && timeEl.current) {
        timeEl.current.textContent = formatTime(position.current);
        last = second;
      }
      frame = requestAnimationFrame(draw);
    };
    frame = requestAnimationFrame(draw);
    return () => cancelAnimationFrame(frame);
  }, [position]);

  const onPad = (slot: CueSlot) => (event: MouseEvent<HTMLButtonElement>) => {
    releasePointerFocus(event);
    if (event.shiftKey) void clearHotCue(slot);
    else void triggerHotCue(slot, position.current);
  };

  return (
    <div className="flex flex-wrap items-center gap-2">
      <Button
        variant="secondary"
        size="lg"
        onClick={(e) => {
          releasePointerFocus(e);
          void cue();
        }}
        title={t("deck.transport.cueTitle")}
      >
        CUE
      </Button>
      <Button
        size="lg"
        onClick={(e) => {
          releasePointerFocus(e);
          void togglePlay();
        }}
        aria-label={playing ? t("deck.transport.pause") : t("deck.transport.play")}
        title={t("deck.transport.playTitle")}
      >
        {playing ? <Pause aria-hidden="true" /> : <Play aria-hidden="true" />}
      </Button>

      <fieldset className="flex items-center gap-1.5">
        <legend className="sr-only">
          {padPage === 1
            ? t("deck.transport.padsLegend", { from: 1, to: 4 })
            : t("deck.transport.padsLegend", { from: 5, to: 8 })}
        </legend>
        {PAGE_SLOTS[padPage].map((slot) => {
          const set = detail.cues.find((c) => c.slot === slot);
          return (
            <button
              key={slot}
              type="button"
              onClick={onPad(slot)}
              aria-label={
                set
                  ? t("deck.transport.padSet", {
                      slot,
                      time: formatTime(set.positionSec),
                    })
                  : t("deck.transport.padEmpty", { slot })
              }
              title={t("deck.transport.padTitle", { slot })}
              className={cn(
                "h-8 w-9 rounded-md font-mono text-[13px] font-bold",
                set
                  ? cn(CUE_BG[set.color], "text-ck-ink")
                  : "border border-dashed border-border text-muted-foreground",
              )}
            >
              {slot}
            </button>
          );
        })}
        <Button
          variant="ghost"
          size="sm"
          onClick={togglePadPage}
          aria-label={
            padPage === 1
              ? t("deck.transport.showPads", { from: 5, to: 8 })
              : t("deck.transport.showPads", { from: 1, to: 4 })
          }
        >
          {padPage === 1 ? "5-8" : "1-4"}
        </Button>
      </fieldset>

      <span className="ml-1 font-mono text-xs tabular-nums">
        <span ref={timeEl}>0:00</span>{" "}
        <span className="text-muted-foreground">
          / {formatTime(detail.track.durationSec)}
        </span>
      </span>

      <span className="ml-auto text-xs text-muted-foreground">
        {t("deck.transport.hearingLabel")}{" "}
        <b className="font-semibold text-primary">
          {t(`deck.transport.hearing.${mode}`)}
        </b>
      </span>
    </div>
  );
}

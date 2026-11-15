import { cn } from "cn";
import { Pause, Play } from "lucide-react";
import { useEffect, useRef, type JSX, type MouseEvent } from "react";

import { Knob } from "@/components/controls/Knob";
import { CUE_BG } from "@/components/deck/cueColors";
import { Button } from "@/components/ui/button";
import { usePlaybackPositionRef } from "@/hooks/usePlaybackRefs";
import { releasePointerFocus } from "@/lib/focus";
import { filterText, formatTime } from "@/lib/format";
import type { CueSlot, PlaybackMode, TrackDetail } from "@/lib/types";
import { usePlayer } from "@/stores/player";
import { useUi } from "@/stores/ui";

const PAGE_SLOTS: Record<1 | 2, readonly CueSlot[]> = {
  1: [1, 2, 3, 4],
  2: [5, 6, 7, 8],
};

const HEARING: Record<PlaybackMode, string> = {
  original: "Original",
  mastered: "Mastered",
  stems: "Stems mix",
};

/** Transport row: CUE, play, hot cue pads (paged), time, what you hear, filter. */
export function Transport({ detail }: { detail: TrackDetail }): JSX.Element {
  const playing = usePlayer((s) => s.playing);
  const mode = usePlayer((s) => s.mode);
  const filter = usePlayer((s) => s.filter);
  const togglePlay = usePlayer((s) => s.togglePlay);
  const cue = usePlayer((s) => s.cue);
  const triggerHotCue = usePlayer((s) => s.triggerHotCue);
  const clearHotCue = usePlayer((s) => s.clearHotCue);
  const setFilter = usePlayer((s) => s.setFilter);
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
        title="Cue (C)"
      >
        CUE
      </Button>
      <Button
        size="lg"
        onClick={(e) => {
          releasePointerFocus(e);
          void togglePlay();
        }}
        aria-label={playing ? "Pause" : "Play"}
        title="Play / pause (Space)"
      >
        {playing ? <Pause aria-hidden="true" /> : <Play aria-hidden="true" />}
      </Button>

      <fieldset className="flex items-center gap-1.5">
        <legend className="sr-only">{`Hot cues ${padPage === 1 ? "1 to 4" : "5 to 8"}`}</legend>
        {PAGE_SLOTS[padPage].map((slot) => {
          const set = detail.cues.find((c) => c.slot === slot);
          return (
            <button
              key={slot}
              type="button"
              onClick={onPad(slot)}
              aria-label={
                set
                  ? `Hot cue ${slot}, jump to ${formatTime(set.positionSec)}`
                  : `Hot cue ${slot}, empty: set at playhead`
              }
              title={`Hot cue ${slot} (${slot}). Shift-click to clear`}
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
          aria-label={`Show hot cues ${padPage === 1 ? "5 to 8" : "1 to 4"}`}
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
        Hearing: <b className="font-semibold text-primary">{HEARING[mode]}</b>
      </span>
      <Knob
        label="Filter"
        value={filter}
        min={-1}
        max={1}
        defaultValue={0}
        bipolar
        valueText={filterText(filter)}
        onChange={(v) => void setFilter(v)}
        size={40}
      />
    </div>
  );
}

import { cn } from "cn";
import { useEffect, useRef, type JSX, type RefObject } from "react";

import { ShortcutList } from "@/components/controls/ShortcutList";
import { ToolCard, ToolEmpty } from "@/components/tools/ToolCard";
import { Button } from "@/components/ui/button";
import { api } from "@/lib/ipc";
import type { ControlTarget, MidiBinding, MidiDevice } from "@/lib/types";
import { useController } from "@/stores/controller";
import { useDevices } from "@/stores/devices";
import { useUi } from "@/stores/ui";

/** How long a control stays lit after Rust reports it moved. */
const ACTIVITY_MS = 160;

/** Display names for control targets. */
const TARGET_LABEL: Record<ControlTarget, string> = {
  play: "Play / pause",
  cue: "Cue",
  "hotcue-1": "Hot cue 1",
  "hotcue-2": "Hot cue 2",
  "hotcue-3": "Hot cue 3",
  "hotcue-4": "Hot cue 4",
  "hotcue-5": "Hot cue 5",
  "hotcue-6": "Hot cue 6",
  "hotcue-7": "Hot cue 7",
  "hotcue-8": "Hot cue 8",
  "pad-page": "Pad page (1-4 / 5-8)",
  "mute-vocals": "Mute vocals",
  "mute-drums": "Mute drums",
  "mute-bass": "Mute bass",
  "mute-other": "Mute other",
  filter: "Filter knob",
  tempo: "Tempo fader",
  volume: "Volume",
  "ab-toggle": "A/B compare",
};

function bindingText(binding: MidiBinding | null): string {
  if (!binding) return "Not mapped";
  return `${binding.kind === "note" ? "Note" : "CC"} ${binding.number}, ch ${binding.channel + 1}`;
}

/**
 * Lights up the drawn control for each control event Rust sends, by setting a
 * data attribute on the DOM node. Knobs and faders can send events very fast,
 * so this never goes through React state.
 */
function useControlActivity(root: RefObject<HTMLElement | null>): void {
  useEffect(() => {
    const timers = new Map<Element, ReturnType<typeof setTimeout>>();
    const sub = api().onControlEvent((event) => {
      const el = root.current?.querySelector(`[data-control="${event.target}"]`);
      if (!el) return;
      el.setAttribute("data-active", "");
      clearTimeout(timers.get(el));
      timers.set(
        el,
        setTimeout(() => el.removeAttribute("data-active"), ACTIVITY_MS),
      );
    });
    return () => {
      void sub.then((stop) => stop());
      for (const timer of timers.values()) clearTimeout(timer);
    };
  }, [root]);
}

/**
 * Controller tab (#40, UI side): a drawing of the connected controller that
 * lights up as you use it, and the MIDI mapping table with MIDI learn. Rust
 * owns the mappings and runs the actions; this panel only shows them.
 */
export function ControllerPanel(): JSX.Element {
  const device = useDevices((s) => s.midi.find((d) => d.connected) ?? null);

  if (!device) {
    return (
      <div className="flex flex-col gap-3 p-3">
        <ToolEmpty>Plug in a controller or use keyboard shortcuts.</ToolEmpty>
        <ShortcutList />
      </div>
    );
  }
  // A different device is a fresh instance: its unmount cancels a pending learn.
  return <ConnectedController key={device.id} device={device} />;
}

function ConnectedController({ device }: { device: MidiDevice }): JSX.Element {
  const mappings = useController((s) => s.mappings);
  const learning = useController((s) => s.learning);
  const lastLearned = useController((s) => s.lastLearned);
  const load = useController((s) => s.load);
  const startLearn = useController((s) => s.startLearn);
  const cancelLearn = useController((s) => s.cancelLearn);
  const drawing = useRef<HTMLDivElement>(null);
  useControlActivity(drawing);

  useEffect(() => {
    void load(device.id);
  }, [device.id, load]);

  // Leaving the tab (or unplugging) stops a pending learn.
  useEffect(() => () => void useController.getState().cancelLearn(), []);

  // Escape cancels a pending learn wherever focus is (WebKit does not focus clicked buttons).
  useEffect(() => {
    if (learning === null) return;
    const onKeyDown = (event: globalThis.KeyboardEvent): void => {
      if (event.key !== "Escape") return;
      event.preventDefault();
      void useController.getState().cancelLearn();
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [learning]);

  return (
    <div className="flex flex-col gap-3 p-3">
      <ToolCard
        title={device.name}
        aside={
          <span className="flex items-center gap-1.5 text-[11px] text-primary">
            <span aria-hidden="true" className="size-1.5 rounded-full bg-primary" />
            connected
          </span>
        }
      >
        <ControllerDrawing root={drawing} />
      </ToolCard>

      <ToolCard title="Mapping">
        <div>
          <table className="w-full">
            <caption className="sr-only">MIDI mapping for {device.name}</caption>
            <thead>
              <tr className="text-left text-[11px] text-muted-foreground">
                <th scope="col" className="pb-1 font-normal">
                  Control
                </th>
                <th scope="col" className="pb-1 font-normal">
                  MIDI
                </th>
                <th scope="col" className="pb-1 font-normal">
                  <span className="sr-only">Learn</span>
                </th>
              </tr>
            </thead>
            <tbody>
              {mappings.map((m) => {
                const listening = learning === m.target;
                const learned = lastLearned?.target === m.target;
                return (
                  <tr key={m.target} className={cn(listening && "bg-primary/10")}>
                    <th scope="row" className="py-1 text-left font-normal">
                      {TARGET_LABEL[m.target]}
                    </th>
                    <td className="py-1 font-mono text-[11px] tabular-nums">
                      {listening ? (
                        <span className="text-primary">Move a control...</span>
                      ) : (
                        <span className={cn(!m.binding && "text-muted-foreground")}>
                          {bindingText(m.binding)}
                        </span>
                      )}
                      {learned && lastLearned.replaced && (
                        <span className="block text-ck-orange">
                          {`Moved from ${TARGET_LABEL[lastLearned.replaced]}`}
                        </span>
                      )}
                    </td>
                    <td className="py-1 text-right">
                      <Button
                        variant={listening ? "default" : "ghost"}
                        size="sm"
                        aria-label={
                          listening
                            ? `Cancel learning ${TARGET_LABEL[m.target]}`
                            : `Learn ${TARGET_LABEL[m.target]}`
                        }
                        onClick={() =>
                          void (listening ? cancelLearn() : startLearn(m.target))
                        }
                      >
                        {listening ? "Cancel" : learned ? "Learned" : "Learn"}
                      </Button>
                    </td>
                  </tr>
                );
              })}
            </tbody>
          </table>
          <output aria-live="polite" className="sr-only">
            {learning !== null
              ? `Move a control on the ${device.name} to map ${TARGET_LABEL[learning]}`
              : lastLearned
                ? `Mapped ${TARGET_LABEL[lastLearned.target]}${lastLearned.replaced ? `, moved from ${TARGET_LABEL[lastLearned.replaced]}` : ""}`
                : ""}
          </output>
        </div>
      </ToolCard>
    </div>
  );
}

/** Lit state for a drawn control (set by useControlActivity). */
const LIT = "data-[active]:border-primary data-[active]:bg-primary/25";

/** Generic two-row layout: jog wheel, filter and tempo, and the 8 pads. */
function ControllerDrawing({
  root,
}: {
  root: RefObject<HTMLDivElement | null>;
}): JSX.Element {
  const padPage = useUi((s) => s.padPage);
  const cueSlots = padPage === 1 ? [1, 2, 3, 4] : [5, 6, 7, 8];
  const stemPads: ReadonlyArray<{ target: ControlTarget; label: string }> = [
    { target: "mute-vocals", label: "Voc" },
    { target: "mute-drums", label: "Drm" },
    { target: "mute-bass", label: "Bas" },
    { target: "mute-other", label: "Oth" },
  ];

  return (
    <figure>
      <figcaption className="sr-only">
        {`Controller layout: jog wheel, filter knob, tempo fader, hot cue pads ${padPage === 1 ? "1 to 4" : "5 to 8"} and stem mute pads. Controls light up as you use them.`}
      </figcaption>
      <div ref={root} aria-hidden="true" className="grid grid-cols-[1fr_auto] gap-3">
        <div className="flex items-center gap-3">
          <div className="grid size-20 shrink-0 place-items-center rounded-full border-2 border-border bg-secondary">
            <div className="size-6 rounded-full border border-border bg-background" />
          </div>
          <div className="flex flex-col items-center gap-1">
            <div
              data-control="filter"
              className={cn(
                "size-8 rounded-full border-2 border-border bg-secondary",
                LIT,
              )}
            />
            <span className="text-[10px] text-muted-foreground">Filter</span>
          </div>
          <div className="flex flex-col gap-1">
            <div
              data-control="cue"
              className={cn("rounded-sm border border-border px-2 py-1 text-[10px]", LIT)}
            >
              CUE
            </div>
            <div
              data-control="play"
              className={cn("rounded-sm border border-border px-2 py-1 text-[10px]", LIT)}
            >
              PLAY
            </div>
            <div
              data-control="pad-page"
              className={cn("rounded-sm border border-border px-2 py-1 text-[10px]", LIT)}
            >
              {padPage === 1 ? "1-4" : "5-8"}
            </div>
          </div>
        </div>
        <div className="flex flex-col items-center gap-1">
          <div
            data-control="tempo"
            className={cn(
              "relative h-20 w-3 rounded-full border border-border bg-secondary",
              LIT,
            )}
          >
            <div className="absolute inset-x-[-4px] top-1/2 h-2 rounded-sm bg-muted-foreground" />
          </div>
          <span className="text-[10px] text-muted-foreground">Tempo</span>
        </div>
        <div className="col-span-2 grid grid-cols-4 gap-1.5">
          {cueSlots.map((slot) => (
            <div
              key={slot}
              data-control={`hotcue-${slot}`}
              className={cn(
                "grid h-8 place-items-center rounded-md border border-border font-mono text-[11px]",
                LIT,
              )}
            >
              {slot}
            </div>
          ))}
          {stemPads.map((pad) => (
            <div
              key={pad.target}
              data-control={pad.target}
              className={cn(
                "grid h-8 place-items-center rounded-md border border-dashed border-border text-[10px] text-muted-foreground",
                LIT,
              )}
            >
              {pad.label}
            </div>
          ))}
        </div>
      </div>
    </figure>
  );
}

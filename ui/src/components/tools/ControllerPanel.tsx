import { cn } from "cn";
import type { TFunction } from "i18next";
import { useEffect, useRef, type JSX, type RefObject } from "react";
import { useTranslation } from "react-i18next";

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

/** Translation keys of the display names for control targets. */
const TARGET_LABEL = {
  play: "tools.controller.targets.play",
  cue: "tools.controller.targets.cue",
  "hotcue-1": "tools.controller.targets.hotcue1",
  "hotcue-2": "tools.controller.targets.hotcue2",
  "hotcue-3": "tools.controller.targets.hotcue3",
  "hotcue-4": "tools.controller.targets.hotcue4",
  "hotcue-5": "tools.controller.targets.hotcue5",
  "hotcue-6": "tools.controller.targets.hotcue6",
  "hotcue-7": "tools.controller.targets.hotcue7",
  "hotcue-8": "tools.controller.targets.hotcue8",
  "pad-page": "tools.controller.targets.padPage",
  "mute-vocals": "tools.controller.targets.muteVocals",
  "mute-drums": "tools.controller.targets.muteDrums",
  "mute-bass": "tools.controller.targets.muteBass",
  "mute-other": "tools.controller.targets.muteOther",
  filter: "tools.controller.targets.filter",
  tempo: "tools.controller.targets.tempo",
  volume: "tools.controller.targets.volume",
  "ab-toggle": "tools.controller.targets.abToggle",
} as const satisfies Record<ControlTarget, string>;

function bindingText(binding: MidiBinding | null, t: TFunction): string {
  if (!binding) return t("tools.controller.notMapped");
  return t("tools.controller.binding", {
    kind: binding.kind === "note" ? t("tools.controller.note") : "CC",
    number: binding.number,
    channel: binding.channel + 1,
  });
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
  const { t } = useTranslation();
  const device = useDevices((s) => s.midi.find((d) => d.connected) ?? null);

  if (!device) {
    return (
      <div className="flex flex-col gap-3 p-3">
        <ToolEmpty>{t("tools.controller.empty")}</ToolEmpty>
        <ShortcutList />
      </div>
    );
  }
  // A different device is a fresh instance: its unmount cancels a pending learn.
  return <ConnectedController key={device.id} device={device} />;
}

function ConnectedController({ device }: { device: MidiDevice }): JSX.Element {
  const { t } = useTranslation();
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
            {t("tools.controller.connected")}
          </span>
        }
      >
        <ControllerDrawing root={drawing} />
      </ToolCard>

      <ToolCard title={t("tools.controller.mapping")}>
        <div>
          <table className="w-full">
            <caption className="sr-only">
              {t("tools.controller.mappingCaption", { device: device.name })}
            </caption>
            <thead>
              <tr className="text-left text-[11px] text-muted-foreground">
                <th scope="col" className="pb-1 font-normal">
                  {t("tools.controller.control")}
                </th>
                <th scope="col" className="pb-1 font-normal">
                  MIDI
                </th>
                <th scope="col" className="pb-1 font-normal">
                  <span className="sr-only">{t("tools.controller.learn")}</span>
                </th>
              </tr>
            </thead>
            <tbody>
              {mappings.map((m) => {
                const listening = learning === m.target;
                const learned = lastLearned?.target === m.target;
                const target = t(TARGET_LABEL[m.target]);
                return (
                  <tr key={m.target} className={cn(listening && "bg-primary/10")}>
                    <th scope="row" className="py-1 text-left font-normal">
                      {target}
                    </th>
                    <td className="py-1 font-mono text-[11px] tabular-nums">
                      {listening ? (
                        <span className="text-primary">
                          {t("tools.controller.moveControl")}
                        </span>
                      ) : (
                        <span className={cn(!m.binding && "text-muted-foreground")}>
                          {bindingText(m.binding, t)}
                        </span>
                      )}
                      {learned && lastLearned.replaced && (
                        <span className="block text-ck-orange">
                          {t("tools.controller.movedFrom", {
                            target: t(TARGET_LABEL[lastLearned.replaced]),
                          })}
                        </span>
                      )}
                    </td>
                    <td className="py-1 text-right">
                      <Button
                        variant={listening ? "default" : "ghost"}
                        size="sm"
                        aria-label={
                          listening
                            ? t("tools.controller.cancelLearnTarget", { target })
                            : t("tools.controller.learnTarget", { target })
                        }
                        onClick={() =>
                          void (listening ? cancelLearn() : startLearn(m.target))
                        }
                      >
                        {listening
                          ? t("tools.controller.cancel")
                          : learned
                            ? t("tools.controller.learned")
                            : t("tools.controller.learn")}
                      </Button>
                    </td>
                  </tr>
                );
              })}
            </tbody>
          </table>
          <output aria-live="polite" className="sr-only">
            {learning !== null
              ? t("tools.controller.listening", {
                  device: device.name,
                  target: t(TARGET_LABEL[learning]),
                })
              : lastLearned
                ? lastLearned.replaced
                  ? t("tools.controller.mappedMoved", {
                      target: t(TARGET_LABEL[lastLearned.target]),
                      from: t(TARGET_LABEL[lastLearned.replaced]),
                    })
                  : t("tools.controller.mapped", {
                      target: t(TARGET_LABEL[lastLearned.target]),
                    })
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
  const { t } = useTranslation();
  const padPage = useUi((s) => s.padPage);
  const cueSlots = padPage === 1 ? [1, 2, 3, 4] : [5, 6, 7, 8];
  const stemPads = [
    { target: "mute-vocals", label: "tools.controller.pads.vocals" },
    { target: "mute-drums", label: "tools.controller.pads.drums" },
    { target: "mute-bass", label: "tools.controller.pads.bass" },
    { target: "mute-other", label: "tools.controller.pads.other" },
  ] as const satisfies ReadonlyArray<{ target: ControlTarget; label: string }>;

  return (
    <figure>
      <figcaption className="sr-only">
        {padPage === 1
          ? t("tools.controller.layout", { first: 1, last: 4 })
          : t("tools.controller.layout", { first: 5, last: 8 })}
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
            <span className="text-[10px] text-muted-foreground">
              {t("tools.controller.filter")}
            </span>
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
          <span className="text-[10px] text-muted-foreground">
            {t("tools.controller.tempo")}
          </span>
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
              {t(pad.label)}
            </div>
          ))}
        </div>
      </div>
    </figure>
  );
}

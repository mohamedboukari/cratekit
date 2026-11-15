import { Download, X } from "lucide-react";
import { useId, type JSX } from "react";
import { toast } from "sonner";

import { Meter } from "@/components/tools/Meter";
import { ToolCard, ToolEmpty } from "@/components/tools/ToolCard";
import { Button } from "@/components/ui/button";
import { useMaster, type AbSide } from "@/stores/master";
import { usePlayer } from "@/stores/player";

/** Output never exceeds this (SPEC, Module 3); Rust enforces it, the UI shows it. */
const CEILING_DBTP = -1;
/** Loudness target range offered by the slider. */
const LUFS_MIN = -16;
const LUFS_MAX = -6;

/** A labelled native slider with its value shown beside the label. */
function RangeField({
  label,
  valueText,
  min,
  max,
  step,
  value,
  onChange,
}: {
  label: string;
  valueText: string;
  min: number;
  max: number;
  step: number;
  value: number;
  onChange: (value: number) => void;
}): JSX.Element {
  const id = useId();
  return (
    <div className="mt-3">
      <div className="flex justify-between">
        <label htmlFor={id}>{label}</label>
        <span aria-hidden="true" className="font-mono tabular-nums">
          {valueText}
        </span>
      </div>
      <input
        id={id}
        type="range"
        min={min}
        max={max}
        step={step}
        value={value}
        aria-valuetext={valueText}
        onChange={(e) => onChange(Number(e.target.value))}
        className="mt-1 w-full accent-primary"
      />
    </div>
  );
}

/** A row of toggle buttons where exactly one is pressed (presets, A/B, bit depth). */
function Segmented<T extends string | number>({
  legend,
  options,
  value,
  onChange,
}: {
  legend: string;
  options: ReadonlyArray<{ value: T; label: string; title?: string }>;
  value: T;
  onChange: (value: T) => void;
}): JSX.Element {
  return (
    <fieldset className="flex min-w-0 flex-wrap gap-1">
      <legend className="sr-only">{legend}</legend>
      {options.map((o) => (
        <Button
          key={o.value}
          variant={o.value === value ? "default" : "outline"}
          size="sm"
          aria-pressed={o.value === value}
          title={o.title}
          onClick={() => onChange(o.value)}
        >
          {o.label}
        </Button>
      ))}
    </fieldset>
  );
}

const AB_OPTIONS: ReadonlyArray<{ value: AbSide; label: string; title: string }> = [
  { value: "a", label: "A: Original", title: "Hear the original (B)" },
  { value: "b", label: "B: Mastered", title: "Hear the master (B)" },
];

const BIT_DEPTHS: ReadonlyArray<{ value: 16 | 24; label: string; title: string }> = [
  { value: 24, label: "24-bit", title: "WAV 24-bit" },
  { value: 16, label: "16-bit", title: "WAV 16-bit with dither" },
];

/**
 * Master tab (#28, UI side): preset, intensity, loudness target, optional
 * reference, level-matched A/B, output meters and WAV export. Rust renders
 * the B side and the export; the UI only sends settings.
 */
export function MasterPanel(): JSX.Element {
  const detail = usePlayer((s) => s.detail);
  const presets = useMaster((s) => s.presets);
  const presetsStatus = useMaster((s) => s.presetsStatus);
  const loadPresets = useMaster((s) => s.loadPresets);
  const settings = useMaster((s) => s.settings);
  const referenceTitle = useMaster((s) => s.referenceTitle);
  const side = useMaster((s) => s.side);
  const levelMatch = useMaster((s) => s.levelMatch);
  const exporting = useMaster((s) => s.exporting);
  const result = useMaster((s) => s.result);
  const setPreset = useMaster((s) => s.setPreset);
  const setIntensity = useMaster((s) => s.setIntensity);
  const setTargetLufs = useMaster((s) => s.setTargetLufs);
  const setReference = useMaster((s) => s.setReference);
  const setBitDepth = useMaster((s) => s.setBitDepth);
  const setSide = useMaster((s) => s.setSide);
  const setLevelMatch = useMaster((s) => s.setLevelMatch);
  const exportMaster = useMaster((s) => s.exportMaster);

  if (!detail) return <ToolEmpty>Load a track to master it.</ToolEmpty>;

  const trackId = detail.track.id;
  const preset = presets.find((p) => p.name === settings.preset);
  const lastExport = result?.trackId === trackId ? result.master : null;

  const onExport = async (): Promise<void> => {
    const master = await exportMaster(trackId);
    if (master) toast.success("Master exported", { description: master.path });
  };

  return (
    <div className="flex flex-col gap-3 p-3">
      <ToolCard title="Sound">
        {presetsStatus === "ready" ? (
          <Segmented
            legend="Preset"
            options={presets.map((p) => ({ value: p.name, label: p.label }))}
            value={settings.preset}
            onChange={setPreset}
          />
        ) : presetsStatus === "loading" ? (
          <p className="text-muted-foreground">Loading presets...</p>
        ) : (
          <p className="flex items-center gap-2 text-muted-foreground">
            Could not load the presets.
            <Button variant="outline" size="sm" onClick={() => void loadPresets()}>
              Retry
            </Button>
          </p>
        )}
        {preset && <p className="mt-1.5 text-muted-foreground">{preset.description}</p>}

        <RangeField
          label="Intensity"
          valueText={`${Math.round(settings.intensity * 100)}%`}
          min={0}
          max={100}
          step={1}
          value={Math.round(settings.intensity * 100)}
          onChange={(v) => setIntensity(v / 100)}
        />
        <RangeField
          label="Loudness target"
          valueText={`${settings.targetLufs.toFixed(1)} LUFS`}
          min={LUFS_MIN}
          max={LUFS_MAX}
          step={0.5}
          value={settings.targetLufs}
          onChange={setTargetLufs}
        />

        <div className="mt-3 flex items-center gap-2">
          <span className="text-muted-foreground">Reference</span>
          {referenceTitle ? (
            <>
              <span className="min-w-0 flex-1 truncate" title={referenceTitle}>
                {referenceTitle}
              </span>
              <Button
                variant="ghost"
                size="icon-xs"
                aria-label="Clear the reference track"
                onClick={() => setReference(null)}
              >
                <X aria-hidden="true" />
              </Button>
            </>
          ) : (
            <span className="text-[11px] text-muted-foreground">
              None. Right-click a track and choose Use as mastering reference.
            </span>
          )}
        </div>
      </ToolCard>

      <ToolCard
        title="Compare"
        aside={
          <label className="flex items-center gap-1.5 text-[11px] text-muted-foreground">
            <input
              type="checkbox"
              checked={levelMatch}
              onChange={(e) => void setLevelMatch(e.target.checked)}
              className="accent-primary"
            />
            Level-matched
          </label>
        }
      >
        <Segmented
          legend="A/B compare"
          options={AB_OPTIONS}
          value={side}
          onChange={setSide}
        />
        <div className="mt-3">
          <Meter ceilingDb={CEILING_DBTP} />
        </div>
      </ToolCard>

      <ToolCard title="Export">
        <p className="text-muted-foreground">
          {`Target ${settings.targetLufs.toFixed(1)} LUFS, ceiling ${CEILING_DBTP.toFixed(1)} dBTP`}
        </p>
        {lastExport && (
          <p className="mt-1">
            <span className="text-primary">Exported:</span>{" "}
            <span className="font-mono tabular-nums">
              {`${lastExport.lufs.toFixed(1)} LUFS, ${lastExport.truePeakDb.toFixed(1)} dBTP`}
            </span>
            <span
              className="block truncate text-[11px] text-muted-foreground"
              title={lastExport.path}
            >
              {lastExport.path}
            </span>
          </p>
        )}
        <div className="mt-2.5 flex flex-wrap items-center gap-2">
          <Segmented
            legend="Bit depth"
            options={BIT_DEPTHS}
            value={settings.bitDepth}
            onChange={setBitDepth}
          />
          <Button
            className="ml-auto"
            disabled={exporting}
            onClick={() => void onExport()}
          >
            <Download aria-hidden="true" data-icon="inline-start" />
            {exporting ? "Rendering..." : "Export WAV"}
          </Button>
        </div>
      </ToolCard>
    </div>
  );
}

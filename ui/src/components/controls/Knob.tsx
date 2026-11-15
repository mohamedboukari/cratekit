import { useRef, type JSX, type KeyboardEvent, type PointerEvent } from "react";

interface KnobProps {
  label: string;
  /** Current value within [min, max]. */
  value: number;
  min: number;
  max: number;
  /** Value restored by double-click, Delete or Backspace. */
  defaultValue: number;
  /** Draw the value arc from the centre (e.g. the DJ filter). */
  bipolar?: boolean;
  /** Screen-reader and tooltip text for the current value. */
  valueText: string;
  onChange: (value: number) => void;
  size?: number;
}

/** Fraction of the range per arrow key press. */
const STEP = 0.05;
const BIG_STEP = 0.2;
/** Vertical drag distance (CSS px) for the full range. */
const DRAG_RANGE_PX = 200;
/** The knob sweeps 270 degrees, from 135 (bottom left) to 405 (bottom right). */
const START_DEG = 135;
const SWEEP_DEG = 270;

function polar(cx: number, cy: number, r: number, deg: number): [number, number] {
  const rad = (deg * Math.PI) / 180;
  return [cx + r * Math.cos(rad), cy + r * Math.sin(rad)];
}

function arc(cx: number, cy: number, r: number, from: number, to: number): string {
  const [x1, y1] = polar(cx, cy, r, from);
  const [x2, y2] = polar(cx, cy, r, to);
  const large = Math.abs(to - from) > 180 ? 1 : 0;
  const sweep = to > from ? 1 : 0;
  return `M ${x1} ${y1} A ${r} ${r} 0 ${large} ${sweep} ${x2} ${y2}`;
}

/**
 * Accessible rotary knob (role="slider"): drag up/down, arrow keys (Shift for
 * bigger steps), Page Up/Down, Home/End, double-click or Delete to reset.
 */
export function Knob({
  label,
  value,
  min,
  max,
  defaultValue,
  bipolar = false,
  valueText,
  onChange,
  size = 44,
}: KnobProps): JSX.Element {
  const drag = useRef<{ startY: number; startValue: number } | null>(null);
  const range = max - min;
  const clamp = (v: number): number => Math.max(min, Math.min(max, v));
  const t = range > 0 ? (value - min) / range : 0;
  const angle = START_DEG + t * SWEEP_DEG;
  const centreAngle =
    START_DEG + (bipolar ? (defaultValue - min) / range : 0) * SWEEP_DEG;
  const c = size / 2;
  const r = size / 2 - 4;
  const [tipX, tipY] = polar(c, c, r - 6, angle);

  const onKeyDown = (event: KeyboardEvent<HTMLDivElement>): void => {
    const step = (event.shiftKey ? BIG_STEP : STEP) * range;
    let next: number | null = null;
    if (event.key === "ArrowUp" || event.key === "ArrowRight") next = value + step;
    else if (event.key === "ArrowDown" || event.key === "ArrowLeft") next = value - step;
    else if (event.key === "PageUp") next = value + BIG_STEP * range;
    else if (event.key === "PageDown") next = value - BIG_STEP * range;
    else if (event.key === "Home") next = min;
    else if (event.key === "End") next = max;
    else if (event.key === "Delete" || event.key === "Backspace") next = defaultValue;
    if (next === null) return;
    event.preventDefault();
    onChange(clamp(next));
  };

  const onPointerDown = (event: PointerEvent<HTMLDivElement>): void => {
    event.currentTarget.setPointerCapture(event.pointerId);
    drag.current = { startY: event.clientY, startValue: value };
  };
  const onPointerMove = (event: PointerEvent<HTMLDivElement>): void => {
    if (!drag.current) return;
    const delta = ((drag.current.startY - event.clientY) / DRAG_RANGE_PX) * range;
    onChange(clamp(drag.current.startValue + delta));
  };
  const onPointerUp = (event: PointerEvent<HTMLDivElement>): void => {
    drag.current = null;
    if (event.currentTarget.hasPointerCapture(event.pointerId)) {
      event.currentTarget.releasePointerCapture(event.pointerId);
    }
  };

  const valueFrom = Math.min(centreAngle, angle);
  const valueTo = Math.max(centreAngle, angle);

  return (
    <div className="flex flex-col items-center gap-1">
      <div
        // Custom widget (rotary knob / waveform scrubber): no native input can render it;
        // it implements the WAI-ARIA slider pattern (keys, aria-value*) itself.
        // oxlint-disable-next-line jsx-a11y/prefer-tag-over-role
        role="slider"
        tabIndex={0}
        aria-label={label}
        aria-valuemin={min}
        aria-valuemax={max}
        aria-valuenow={Number(value.toFixed(2))}
        aria-valuetext={valueText}
        title={`${label}: ${valueText}`}
        onKeyDown={onKeyDown}
        onPointerDown={onPointerDown}
        onPointerMove={onPointerMove}
        onPointerUp={onPointerUp}
        onPointerCancel={onPointerUp}
        onDoubleClick={() => onChange(defaultValue)}
        className="touch-none rounded-full"
        style={{ width: size, height: size }}
      >
        <svg
          width={size}
          height={size}
          viewBox={`0 0 ${size} ${size}`}
          aria-hidden="true"
        >
          <circle
            cx={c}
            cy={c}
            r={r - 3}
            className="fill-ck-panel2 stroke-ck-line"
            strokeWidth={1}
          />
          <path
            d={arc(c, c, r, START_DEG, START_DEG + SWEEP_DEG)}
            className="fill-none stroke-ck-line"
            strokeWidth={3}
            strokeLinecap="round"
          />
          {valueTo - valueFrom > 0.5 && (
            <path
              d={arc(c, c, r, valueFrom, valueTo)}
              className="fill-none stroke-primary"
              strokeWidth={3}
              strokeLinecap="round"
            />
          )}
          <line
            x1={c}
            y1={c}
            x2={tipX}
            y2={tipY}
            className="stroke-foreground"
            strokeWidth={2}
            strokeLinecap="round"
          />
        </svg>
      </div>
      <span className="text-[11px] tracking-wider text-muted-foreground uppercase">
        {label}
      </span>
    </div>
  );
}

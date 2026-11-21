import { cn } from "cn";
import {
  useEffect,
  useEffectEvent,
  useRef,
  type JSX,
  type KeyboardEvent,
  type PointerEvent,
} from "react";
import { useTranslation } from "react-i18next";

import { isMacPlatform } from "@/lib/shortcuts";

interface KnobProps {
  label: string;
  /** Current value within [min, max]. */
  value: number;
  min: number;
  max: number;
  /** Value restored by double-click, Cmd/Ctrl+click, Delete or Backspace. */
  defaultValue: number;
  /** Draw the value arc from the centre (e.g. the DJ filter). */
  bipolar?: boolean;
  /** Snap to the default near it, like the centre click of a hardware knob. */
  detent?: boolean;
  /** Short labels at the two ends of the arc (e.g. "LP" and "HP"). */
  startLabel?: string;
  endLabel?: string;
  /** The current value in words, shown under the knob and read by screen readers. */
  valueText: string;
  onChange: (value: number) => void;
  size?: number;
}

/** Fraction of the range per arrow key press. */
const STEP = 0.05;
const BIG_STEP = 0.2;
/** Drag distance (CSS px, up or right) for the full range. */
const DRAG_RANGE_PX = 240;
/** Scroll distance (CSS px of wheel or trackpad delta) for the full range. */
const WHEEL_RANGE_PX = 600;
/** Shift slows drags and scrolling down to this fraction, for fine moves. */
const FINE = 0.25;
/** Within this fraction of the range from the default, the detent snaps to it. */
const DETENT = 0.02;
/** Scroll events further apart than this (ms) start a new gesture. */
const WHEEL_GESTURE_MS = 250;
/**
 * Width of the value line, in characters of its mono font: the longest value
 * ("Passe-haut 100 %") fits, so a changing value never resizes the control and
 * what sits beside it (the waveform would redraw and flicker).
 */
const VALUE_CHARS = 16;
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
 * The fraction of the travel (0 to 1) at a point on the dial, from its angle
 * around the centre; null in the gap at the bottom, which has no value (a
 * press there must not jump to an end: a full low-pass or high-pass).
 */
function fractionAt(dx: number, dy: number): number | null {
  const deg = (Math.atan2(dy, dx) * 180) / Math.PI;
  const along = (((deg - START_DEG) % 360) + 360) % 360;
  return along <= SWEEP_DEG ? along / SWEEP_DEG : null;
}

/**
 * The scroll amount of a wheel or trackpad event, positive for "more": fingers
 * or wheel moving up or right. macOS natural scrolling reports deltas inverted
 * from the fingers; WebKit says so, and the sign is turned back.
 */
function wheelAmount(event: WheelEvent): number {
  const inverted =
    "webkitDirectionInvertedFromDevice" in event &&
    event.webkitDirectionInvertedFromDevice === true;
  const unit =
    event.deltaMode === WheelEvent.DOM_DELTA_LINE
      ? 16
      : event.deltaMode === WheelEvent.DOM_DELTA_PAGE
        ? WHEEL_RANGE_PX
        : 1;
  // With Shift, macOS and Chromium turn a vertical wheel into deltaX: it is
  // still the wheel's up and down.
  const shiftedWheel = event.shiftKey && event.deltaY === 0;
  const up = -(shiftedWheel ? event.deltaX : event.deltaY) * unit;
  const right = shiftedWheel ? 0 : event.deltaX * unit;
  const amount = Math.abs(right) > Math.abs(up) ? right : up;
  return inverted ? -amount : amount;
}

/** Cmd+click on macOS (where Control+click is a right click), Ctrl+click elsewhere. */
function isResetClick(event: PointerEvent<HTMLDivElement>): boolean {
  return isMacPlatform() ? event.metaKey : event.ctrlKey;
}

/**
 * Accessible rotary knob (role="slider"), the whole control (knob, labels,
 * value) being the target: click the ring to jump there, drag up, down, left
 * or right, scroll with a trackpad or wheel (Shift for fine moves), arrow keys
 * (Shift for bigger steps), Page Up/Down, Home/End; double-click,
 * Cmd+click (Ctrl+click on Windows and Linux) or Delete resets it.
 */
export function Knob({
  label,
  value,
  min,
  max,
  defaultValue,
  bipolar = false,
  detent = false,
  startLabel,
  endLabel,
  valueText,
  onChange,
  size = 44,
}: KnobProps): JSX.Element {
  const { t } = useTranslation();
  const control = useRef<HTMLDivElement>(null);
  const dial = useRef<SVGSVGElement>(null);
  /** The unsnapped value of a drag, so the detent never traps it. */
  const drag = useRef<{ x: number; y: number; raw: number } | null>(null);
  /** The same for a scroll gesture (trackpad momentum keeps one going). */
  const wheel = useRef<{ raw: number; at: number } | null>(null);
  const range = max - min;
  const clamp = (v: number): number => Math.max(min, Math.min(max, v));
  const snap = (v: number): number =>
    detent && Math.abs(v - defaultValue) < DETENT * range ? defaultValue : v;
  const fraction = range > 0 ? (value - min) / range : 0;
  const angle = START_DEG + fraction * SWEEP_DEG;
  const centreAngle =
    START_DEG + (bipolar ? (defaultValue - min) / range : 0) * SWEEP_DEG;
  const stroke = Math.max(3, Math.round(size / 16));
  const c = size / 2;
  const r = size / 2 - stroke - 1;
  const [tipX, tipY] = polar(c, c, r - stroke * 2, angle);
  const changed = Math.abs(value - defaultValue) > 1e-9;

  /**
   * Sets a value from a key or a reset, ending any drag or scroll gesture, so
   * trackpad momentum still arriving cannot carry on from before it.
   */
  const setDirectly = (next: number): void => {
    drag.current = null;
    wheel.current = null;
    onChange(next);
  };

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
    setDirectly(clamp(next));
  };

  /** A press on the ring (not the centre) sets the value there; returns it. */
  const jumpTo = (event: PointerEvent<HTMLDivElement>): number | null => {
    const box = dial.current?.getBoundingClientRect();
    if (!box || box.width === 0) return null;
    const dx = event.clientX - (box.left + box.width / 2);
    const dy = event.clientY - (box.top + box.height / 2);
    const scale = box.width / size;
    const distance = Math.hypot(dx, dy) / scale;
    // From the face's edge out: a press on the face itself only drags.
    const onRing = distance >= r - stroke && distance <= size / 2 + stroke;
    const at = onRing ? fractionAt(dx, dy) : null;
    if (at === null) return null;
    const next = min + at * range;
    onChange(snap(next));
    return next;
  };

  const onPointerDown = (event: PointerEvent<HTMLDivElement>): void => {
    if (event.button !== 0 || (isMacPlatform() && event.ctrlKey)) return;
    if (isResetClick(event)) {
      setDirectly(defaultValue);
      return;
    }
    event.currentTarget.setPointerCapture(event.pointerId);
    // The second press of a double-click does not jump again: it resets.
    const jumped = event.detail > 1 ? null : jumpTo(event);
    drag.current = { x: event.clientX, y: event.clientY, raw: jumped ?? value };
  };

  const onPointerMove = (event: PointerEvent<HTMLDivElement>): void => {
    const current = drag.current;
    if (!current) return;
    // The button was released where we could not see it (another app).
    if ((event.buttons & 1) === 0) {
      drag.current = null;
      return;
    }
    const moved = event.clientX - current.x - (event.clientY - current.y);
    const scale = event.shiftKey ? FINE : 1;
    current.raw = clamp(current.raw + (moved / DRAG_RANGE_PX) * range * scale);
    current.x = event.clientX;
    current.y = event.clientY;
    onChange(snap(current.raw));
  };
  const onPointerUp = (event: PointerEvent<HTMLDivElement>): void => {
    drag.current = null;
    if (event.currentTarget.hasPointerCapture(event.pointerId)) {
      event.currentTarget.releasePointerCapture(event.pointerId);
    }
  };

  // React's wheel listener is passive, so it cannot keep the pane from
  // scrolling: a native one is added instead.
  const onWheel = useEffectEvent((event: WheelEvent) => {
    event.preventDefault();
    // Ctrl+wheel is a trackpad pinch or the zoom gesture: not a turn.
    if (event.ctrlKey) return;
    const amount = wheelAmount(event);
    if (amount === 0) return;
    const now = event.timeStamp;
    const gesture = wheel.current;
    const raw = gesture && now - gesture.at < WHEEL_GESTURE_MS ? gesture.raw : value;
    const scale = event.shiftKey ? FINE : 1;
    const next = clamp(raw + (amount / WHEEL_RANGE_PX) * range * scale);
    wheel.current = { raw: next, at: now };
    onChange(snap(next));
  });
  useEffect(() => {
    const element = control.current;
    if (!element) return;
    const listener = (event: WheelEvent): void => onWheel(event);
    element.addEventListener("wheel", listener, { passive: false });
    return () => element.removeEventListener("wheel", listener);
  }, []);

  const valueFrom = Math.min(centreAngle, angle);
  const valueTo = Math.max(centreAngle, angle);

  return (
    <div
      ref={control}
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
      title={t("deck.knob.title", { label, value: valueText })}
      onKeyDown={onKeyDown}
      onPointerDown={onPointerDown}
      onPointerMove={onPointerMove}
      onPointerUp={onPointerUp}
      onPointerCancel={onPointerUp}
      onLostPointerCapture={() => {
        drag.current = null;
      }}
      // Control+click on macOS is a right click: no webview menu over the knob.
      onContextMenu={(event) => event.preventDefault()}
      onDoubleClick={() => setDirectly(defaultValue)}
      className="group flex touch-none flex-col items-center rounded-lg px-1.5 py-1"
    >
      <svg
        ref={dial}
        width={size}
        height={size}
        viewBox={`0 0 ${size} ${size}`}
        aria-hidden="true"
      >
        <circle
          cx={c}
          cy={c}
          r={r - stroke}
          className="fill-ck-panel2 stroke-ck-line transition-colors duration-100 group-hover:stroke-ck-muted motion-reduce:transition-none"
          strokeWidth={1}
        />
        <path
          d={arc(c, c, r, START_DEG, START_DEG + SWEEP_DEG)}
          className="fill-none stroke-ck-line"
          strokeWidth={stroke}
          strokeLinecap="round"
        />
        {valueTo - valueFrom > 0.5 && (
          <path
            d={arc(c, c, r, valueFrom, valueTo)}
            className="fill-none stroke-primary"
            strokeWidth={stroke}
            strokeLinecap="round"
          />
        )}
        <line
          x1={c}
          y1={c}
          x2={tipX}
          y2={tipY}
          className="stroke-foreground"
          strokeWidth={Math.max(2, stroke - 2)}
          strokeLinecap="round"
        />
      </svg>
      {(startLabel ?? endLabel) !== undefined && (
        <div
          aria-hidden="true"
          className="-mt-2.5 flex justify-between px-0.5 text-[11px] text-muted-foreground"
          style={{ width: size }}
        >
          <span>{startLabel}</span>
          <span>{endLabel}</span>
        </div>
      )}
      <span className="mt-0.5 text-[11px] tracking-wider text-muted-foreground uppercase">
        {label}
      </span>
      <span
        aria-hidden="true"
        className={cn(
          "truncate text-center font-mono text-[11px] tabular-nums",
          changed ? "text-primary" : "text-muted-foreground",
        )}
        style={{ width: `${VALUE_CHARS}ch` }}
      >
        {valueText}
      </span>
    </div>
  );
}

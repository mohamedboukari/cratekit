/** Display formatting only (no domain logic). */

/** 0:00 style time; negative values show as 0:00. */
export function formatTime(sec: number): string {
  const s = Math.max(0, Math.floor(sec));
  return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, "0")}`;
}

/** "1 h 12 min" for a total duration in seconds. */
export function formatDuration(sec: number): string {
  const minutes = Math.round(sec / 60);
  const h = Math.floor(minutes / 60);
  const m = minutes % 60;
  return h > 0 ? `${h} h ${m} min` : `${m} min`;
}

/** Filter knob text: "Off", "Low-pass 40%" or "High-pass 25%". */
export function filterText(value: number): string {
  const pct = Math.round(Math.abs(value) * 100);
  if (pct < 2) return "Off";
  return value < 0 ? `Low-pass ${pct}%` : `High-pass ${pct}%`;
}

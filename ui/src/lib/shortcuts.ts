/**
 * The single keyboard shortcut map (docs/SPEC.md, Module 5 keyboard fallback).
 * Menus, tooltips and Settings > Keyboard read labels from here; components
 * never add their own keydown listeners (see hooks/useShortcuts.ts).
 *
 * Keys use KeyboardEvent.code (physical keys), so pads and stem keys stay in
 * the same place on every keyboard layout.
 */

export type ActionId =
  | "play-pause"
  | "cue"
  | "hotcue-1"
  | "hotcue-2"
  | "hotcue-3"
  | "hotcue-4"
  | "hotcue-5"
  | "hotcue-6"
  | "hotcue-7"
  | "hotcue-8"
  | "mute-vocals"
  | "mute-drums"
  | "mute-bass"
  | "mute-other"
  | "ab-toggle"
  | "open-settings";

export interface Shortcut {
  id: ActionId;
  code: string;
  /** Requires Cmd on macOS, Ctrl elsewhere. */
  mod: boolean;
  label: string;
  /** Text shown for the key, e.g. "Space" or "1". */
  keyLabel: string;
}

const HOTCUES: ReadonlyArray<readonly [ActionId, number]> = [
  ["hotcue-1", 1],
  ["hotcue-2", 2],
  ["hotcue-3", 3],
  ["hotcue-4", 4],
  ["hotcue-5", 5],
  ["hotcue-6", 6],
  ["hotcue-7", 7],
  ["hotcue-8", 8],
];

export const SHORTCUTS: readonly Shortcut[] = [
  {
    id: "play-pause",
    code: "Space",
    mod: false,
    label: "Play / pause",
    keyLabel: "Space",
  },
  { id: "cue", code: "KeyC", mod: false, label: "Cue", keyLabel: "C" },
  ...HOTCUES.map(([id, n]): Shortcut => ({
    id,
    code: `Digit${n}`,
    mod: false,
    label: `Hot cue ${n}`,
    keyLabel: String(n),
  })),
  { id: "mute-vocals", code: "KeyQ", mod: false, label: "Mute vocals", keyLabel: "Q" },
  { id: "mute-drums", code: "KeyW", mod: false, label: "Mute drums", keyLabel: "W" },
  { id: "mute-bass", code: "KeyE", mod: false, label: "Mute bass", keyLabel: "E" },
  { id: "mute-other", code: "KeyR", mod: false, label: "Mute other", keyLabel: "R" },
  { id: "ab-toggle", code: "KeyB", mod: false, label: "Compare A/B", keyLabel: "B" },
  { id: "open-settings", code: "Comma", mod: true, label: "Settings", keyLabel: "," },
];

export function isMacPlatform(): boolean {
  return (
    typeof navigator !== "undefined" &&
    /Mac|iPhone|iPad/.test(navigator.platform || navigator.userAgent)
  );
}

/** True when focus is somewhere the user types, so shortcuts must not fire. */
export function isTypingTarget(target: EventTarget | null): boolean {
  if (!(target instanceof HTMLElement)) return false;
  if (target.isContentEditable) return true;
  const tag = target.tagName;
  if (tag === "TEXTAREA" || tag === "SELECT") return true;
  if (tag === "INPUT") {
    const type = target.getAttribute("type") ?? "text";
    return ![
      "checkbox",
      "radio",
      "range",
      "button",
      "submit",
      "reset",
      "color",
      "file",
    ].includes(type);
  }
  return false;
}

/** Controls that use Space themselves (activate, toggle, select). */
const SPACE_ROLES = new Set([
  "button",
  "checkbox",
  "radio",
  "switch",
  "option",
  "tab",
  "menuitem",
]);

/** True when Space should go to the focused control instead of a shortcut. */
export function ownsSpace(target: EventTarget | null): boolean {
  if (!(target instanceof HTMLElement)) return false;
  const tag = target.tagName;
  if (tag === "BUTTON" || tag === "SUMMARY" || tag === "A") return true;
  if (tag === "INPUT") return true;
  const role = target.getAttribute("role");
  return role !== null && SPACE_ROLES.has(role);
}

/** The action for a key event, or null. Ignores typing targets and auto-repeat. */
export function matchShortcut(event: KeyboardEvent, isMac: boolean): ActionId | null {
  if (event.repeat || isTypingTarget(event.target)) return null;
  if (event.code === "Space" && ownsSpace(event.target)) return null;
  const modPressed = isMac ? event.metaKey : event.ctrlKey;
  const otherMod = isMac ? event.ctrlKey : event.metaKey;
  // No shortcut uses Alt, Shift or the other platform's modifier.
  if (event.altKey || event.shiftKey || otherMod) return null;
  const hit = SHORTCUTS.find((s) => s.code === event.code && s.mod === modPressed);
  return hit?.id ?? null;
}

/** "⌘," on macOS, "Ctrl+," elsewhere, or just the key. */
export function formatShortcut(shortcut: Shortcut, isMac: boolean): string {
  if (!shortcut.mod) return shortcut.keyLabel;
  return isMac ? `⌘${shortcut.keyLabel}` : `Ctrl+${shortcut.keyLabel}`;
}

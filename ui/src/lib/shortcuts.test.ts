import { describe, expect, it } from "vitest";

import { formatShortcut, matchShortcut, SHORTCUTS } from "@/lib/shortcuts";

function key(
  code: string,
  init: KeyboardEventInit = {},
  target?: HTMLElement,
): KeyboardEvent {
  const event = new KeyboardEvent("keydown", { code, bubbles: true, ...init });
  if (target) Object.defineProperty(event, "target", { value: target });
  return event;
}

describe("shortcuts", () => {
  it("maps the SPEC keys", () => {
    expect(matchShortcut(key("Space"), false)).toBe("play-pause");
    expect(matchShortcut(key("KeyC"), false)).toBe("cue");
    expect(matchShortcut(key("Digit8"), false)).toBe("hotcue-8");
    expect(matchShortcut(key("KeyQ"), false)).toBe("mute-vocals");
    expect(matchShortcut(key("KeyB"), false)).toBe("ab-toggle");
  });

  it("uses Cmd on macOS and Ctrl elsewhere for Settings", () => {
    expect(matchShortcut(key("Comma", { metaKey: true }), true)).toBe("open-settings");
    expect(matchShortcut(key("Comma", { ctrlKey: true }), false)).toBe("open-settings");
    expect(matchShortcut(key("Comma", { ctrlKey: true }), true)).toBeNull();
    expect(matchShortcut(key("Comma"), false)).toBeNull();
  });

  it("never fires while typing or on auto-repeat or with Shift/Alt", () => {
    const input = document.createElement("input");
    expect(matchShortcut(key("Space", {}, input), false)).toBeNull();
    expect(matchShortcut(key("Space", { repeat: true }), false)).toBeNull();
    expect(matchShortcut(key("Space", { shiftKey: true }), false)).toBeNull();
    expect(matchShortcut(key("KeyC", { altKey: true }), false)).toBeNull();
  });

  it("leaves Space to focused buttons and toggles, but other keys still work there", () => {
    const button = document.createElement("button");
    const checkbox = document.createElement("input");
    checkbox.type = "checkbox";
    const switchEl = document.createElement("div");
    switchEl.setAttribute("role", "switch");
    expect(matchShortcut(key("Space", {}, button), false)).toBeNull();
    expect(matchShortcut(key("Space", {}, checkbox), false)).toBeNull();
    expect(matchShortcut(key("Space", {}, switchEl), false)).toBeNull();
    expect(matchShortcut(key("KeyC", {}, button), false)).toBe("cue");
    expect(matchShortcut(key("Digit2", {}, checkbox), false)).toBe("hotcue-2");
  });

  it("has unique keys and formats modifiers per platform", () => {
    const combos = SHORTCUTS.map((s) => `${s.mod}-${s.code}`);
    expect(new Set(combos).size).toBe(combos.length);
    const settings = SHORTCUTS.find((s) => s.id === "open-settings");
    expect(settings && formatShortcut(settings, true)).toBe("⌘,");
    expect(settings && formatShortcut(settings, false)).toBe("Ctrl+,");
  });
});

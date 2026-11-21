import { fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { Knob } from "@/components/controls/Knob";
import { isMacPlatform } from "@/lib/shortcuts";

/** The reset click's modifier: Cmd on macOS, Ctrl elsewhere. */
const RESET_KEY = isMacPlatform() ? { metaKey: true } : { ctrlKey: true };

function setup(value = 0): { onChange: ReturnType<typeof vi.fn>; knob: HTMLElement } {
  const onChange = vi.fn();
  render(
    <Knob
      label="Filter"
      value={value}
      min={-1}
      max={1}
      defaultValue={0}
      bipolar
      detent
      startLabel="LP"
      endLabel="HP"
      valueText="Off"
      onChange={onChange}
    />,
  );
  return { onChange, knob: screen.getByRole("slider", { name: "Filter" }) };
}

/**
 * A wheel event as the webview sends it, at `at` ms, with WebKit's
 * natural-scrolling flag when given.
 */
function wheel(
  target: HTMLElement,
  at: number,
  init: WheelEventInit,
  inverted?: boolean,
): WheelEvent {
  const event = new WheelEvent("wheel", { bubbles: true, cancelable: true, ...init });
  Object.defineProperty(event, "timeStamp", { value: at });
  if (inverted !== undefined)
    Object.defineProperty(event, "webkitDirectionInvertedFromDevice", {
      value: inverted,
    });
  target.dispatchEvent(event);
  return event;
}

describe("Knob", () => {
  beforeEach(() => {
    // jsdom has no pointer capture.
    Object.assign(Element.prototype, {
      setPointerCapture: vi.fn(),
      hasPointerCapture: vi.fn(() => false),
      releasePointerCapture: vi.fn(),
    });
  });

  afterEach(() => {
    Reflect.deleteProperty(Element.prototype, "setPointerCapture");
    Reflect.deleteProperty(Element.prototype, "hasPointerCapture");
    Reflect.deleteProperty(Element.prototype, "releasePointerCapture");
  });

  it("exposes slider semantics and shows its value and ends", () => {
    const { knob } = setup(0.5);
    expect(knob).toHaveAttribute("aria-valuemin", "-1");
    expect(knob).toHaveAttribute("aria-valuemax", "1");
    expect(knob).toHaveAttribute("aria-valuenow", "0.5");
    expect(knob).toHaveAttribute("aria-valuetext", "Off");
    expect(knob).toHaveTextContent("LP");
    expect(knob).toHaveTextContent("HP");
    expect(knob).toHaveTextContent("Off");
  });

  it("steps with arrow keys, bigger with Shift, and clamps to the range", () => {
    const { onChange, knob } = setup(0);
    fireEvent.keyDown(knob, { key: "ArrowUp" });
    expect(onChange).toHaveBeenLastCalledWith(expect.closeTo(0.1, 5));
    fireEvent.keyDown(knob, { key: "ArrowDown", shiftKey: true });
    expect(onChange).toHaveBeenLastCalledWith(expect.closeTo(-0.4, 5));
    fireEvent.keyDown(knob, { key: "End" });
    expect(onChange).toHaveBeenLastCalledWith(1);
  });

  it("resets to the default with Delete, a double-click or Cmd+click (Ctrl+click off macOS)", () => {
    const { onChange, knob } = setup(0.7);
    fireEvent.keyDown(knob, { key: "Delete" });
    expect(onChange).toHaveBeenLastCalledWith(0);
    onChange.mockClear();
    fireEvent.doubleClick(knob);
    expect(onChange).toHaveBeenLastCalledWith(0);
    onChange.mockClear();
    fireEvent.pointerDown(knob, { button: 0, ...RESET_KEY, clientX: 0, clientY: 0 });
    expect(onChange).toHaveBeenLastCalledWith(0);
    // A reset click starts no drag.
    onChange.mockClear();
    fireEvent.pointerMove(knob, { buttons: 1, clientX: 0, clientY: -50 });
    expect(onChange).not.toHaveBeenCalled();
  });

  it("turns with a drag up or to the right, finer with Shift", () => {
    const { onChange, knob } = setup(0);
    fireEvent.pointerDown(knob, { button: 0, clientX: 100, clientY: 100 });
    fireEvent.pointerMove(knob, { buttons: 1, clientX: 100, clientY: 76 }); // 24 px up: 10% of the travel
    expect(onChange).toHaveBeenLastCalledWith(expect.closeTo(0.2, 5));
    fireEvent.pointerMove(knob, { buttons: 1, clientX: 124, clientY: 76 }); // 24 px right
    expect(onChange).toHaveBeenLastCalledWith(expect.closeTo(0.4, 5));
    fireEvent.pointerMove(knob, {
      buttons: 1,
      clientX: 148,
      clientY: 76,
      shiftKey: true,
    }); // fine
    expect(onChange).toHaveBeenLastCalledWith(expect.closeTo(0.45, 5));
    fireEvent.pointerUp(knob);
  });

  it("clicks into the default near it, and leaves it when the drag goes on", () => {
    const { onChange, knob } = setup(0);
    fireEvent.pointerDown(knob, { button: 0, clientX: 100, clientY: 100 });
    fireEvent.pointerMove(knob, { buttons: 1, clientX: 103, clientY: 100 }); // 0.025: inside the detent
    expect(onChange).toHaveBeenLastCalledWith(0);
    fireEvent.pointerMove(knob, { buttons: 1, clientX: 106, clientY: 100 }); // 0.05: past it
    expect(onChange).toHaveBeenLastCalledWith(expect.closeTo(0.05, 5));
    fireEvent.pointerUp(knob);
  });

  it("jumps to the spot clicked on the ring, and keeps turning from there", () => {
    const { onChange, knob } = setup(0);
    const dial = knob.querySelector("svg");
    if (!dial) throw new Error("no dial");
    // A 44 px dial at the top left corner: centre (22, 22), ring about 17 px out.
    vi.spyOn(dial, "getBoundingClientRect").mockReturnValue(new DOMRect(0, 0, 44, 44));

    fireEvent.pointerDown(knob, { button: 0, clientX: 39, clientY: 22 }); // right of centre
    expect(onChange).toHaveBeenLastCalledWith(expect.closeTo(2 / 3, 5));
    fireEvent.pointerMove(knob, { buttons: 1, clientX: 39, clientY: -2 }); // then 24 px up
    expect(onChange).toHaveBeenLastCalledWith(expect.closeTo(2 / 3 + 0.2, 5));
    fireEvent.pointerUp(knob);

    fireEvent.pointerDown(knob, { button: 0, clientX: 5, clientY: 22 }); // left of centre
    expect(onChange).toHaveBeenLastCalledWith(expect.closeTo(-2 / 3, 5));
    fireEvent.pointerUp(knob);

    fireEvent.pointerDown(knob, { button: 0, clientX: 22, clientY: 5 }); // the top: the centre value
    expect(onChange).toHaveBeenLastCalledWith(0);
    fireEvent.pointerUp(knob);

    // A press in the middle turns by dragging only, without a jump.
    onChange.mockClear();
    fireEvent.pointerDown(knob, { button: 0, clientX: 22, clientY: 22 });
    expect(onChange).not.toHaveBeenCalled();
    fireEvent.pointerUp(knob);
  });

  it("does not jump from a press on the face or the gap at the bottom", () => {
    const { onChange, knob } = setup(0.3);
    const dial = knob.querySelector("svg");
    if (!dial) throw new Error("no dial");
    vi.spyOn(dial, "getBoundingClientRect").mockReturnValue(new DOMRect(0, 0, 44, 44));
    fireEvent.pointerDown(knob, { button: 0, clientX: 22, clientY: 39 }); // the gap, below centre
    fireEvent.pointerUp(knob);
    fireEvent.pointerDown(knob, { button: 0, clientX: 32, clientY: 22 }); // on the face
    fireEvent.pointerUp(knob);
    expect(onChange).not.toHaveBeenCalled();
  });

  it("finds the ring at any rendered size", () => {
    const onChange = vi.fn();
    render(
      <Knob
        label="Big"
        value={0}
        min={-1}
        max={1}
        defaultValue={0}
        valueText="Off"
        onChange={onChange}
        size={80}
      />,
    );
    const knob = screen.getByRole("slider", { name: "Big" });
    const dial = knob.querySelector("svg");
    if (!dial) throw new Error("no dial");
    // Drawn at twice its size: the ring (about 34 px out at 80 px) is 68 px out.
    vi.spyOn(dial, "getBoundingClientRect").mockReturnValue(new DOMRect(0, 0, 160, 160));
    fireEvent.pointerDown(knob, { button: 0, clientX: 148, clientY: 80 });
    expect(onChange).toHaveBeenLastCalledWith(expect.closeTo(2 / 3, 5));
  });

  it("stops a drag whose release it never saw", () => {
    const { onChange, knob } = setup(0);
    fireEvent.pointerDown(knob, { button: 0, clientX: 100, clientY: 100 });
    fireEvent.pointerMove(knob, { buttons: 1, clientX: 100, clientY: 76 });
    expect(onChange).toHaveBeenLastCalledWith(expect.closeTo(0.2, 5));
    // The button came up in another app: moving over the knob changes nothing.
    onChange.mockClear();
    fireEvent.pointerMove(knob, { buttons: 0, clientX: 100, clientY: 10 });
    expect(onChange).not.toHaveBeenCalled();
  });

  it("keeps a reset made during a scroll gesture (trackpad momentum)", () => {
    const { onChange, knob } = setup(0);
    wheel(knob, 1000, { deltaY: -60 });
    wheel(knob, 1016, { deltaY: -60 });
    expect(onChange).toHaveBeenLastCalledWith(expect.closeTo(0.4, 5));
    fireEvent.doubleClick(knob);
    expect(onChange).toHaveBeenLastCalledWith(0);
    // Momentum still arriving starts again from the reset value, not from 0.4.
    wheel(knob, 1032, { deltaY: -60 });
    expect(onChange).toHaveBeenLastCalledWith(expect.closeTo(0.2, 5));
  });

  it("turns with the trackpad or wheel, the fingers' way, without scrolling the pane", () => {
    const { onChange, knob } = setup(0);
    const up = wheel(knob, 1000, { deltaY: -60 });
    expect(up.defaultPrevented).toBe(true);
    expect(onChange).toHaveBeenLastCalledWith(expect.closeTo(0.2, 5));
    // The same gesture goes on from where it got to.
    wheel(knob, 1016, { deltaY: -60 });
    expect(onChange).toHaveBeenLastCalledWith(expect.closeTo(0.4, 5));

    // macOS natural scrolling: the same finger movement arrives with the opposite sign.
    onChange.mockClear();
    wheel(knob, 2000, { deltaY: 60 }, true);
    expect(onChange).toHaveBeenLastCalledWith(expect.closeTo(0.2, 5));

    // A sideways swipe to the right turns it up too.
    onChange.mockClear();
    wheel(knob, 3000, { deltaX: 60, deltaY: 2 });
    expect(onChange).toHaveBeenLastCalledWith(expect.closeTo(0.2, 5));

    // Shift+wheel up arrives as deltaX: still up, and four times finer.
    onChange.mockClear();
    wheel(knob, 4000, { deltaX: -60, deltaY: 0, shiftKey: true });
    expect(onChange).toHaveBeenLastCalledWith(expect.closeTo(0.05, 5));

    // A pinch (Ctrl+wheel) turns nothing.
    onChange.mockClear();
    wheel(knob, 5000, { deltaY: -60, ctrlKey: true });
    expect(onChange).not.toHaveBeenCalled();
  });
});

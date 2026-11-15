import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { Knob } from "@/components/controls/Knob";

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
      valueText="Off"
      onChange={onChange}
    />,
  );
  return { onChange, knob: screen.getByRole("slider", { name: "Filter" }) };
}

describe("Knob", () => {
  it("exposes slider semantics", () => {
    const { knob } = setup(0.5);
    expect(knob).toHaveAttribute("aria-valuemin", "-1");
    expect(knob).toHaveAttribute("aria-valuemax", "1");
    expect(knob).toHaveAttribute("aria-valuenow", "0.5");
    expect(knob).toHaveAttribute("aria-valuetext", "Off");
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

  it("resets to the default with Delete or a double-click", () => {
    const { onChange, knob } = setup(0.7);
    fireEvent.keyDown(knob, { key: "Delete" });
    expect(onChange).toHaveBeenLastCalledWith(0);
    onChange.mockClear();
    fireEvent.doubleClick(knob);
    expect(onChange).toHaveBeenLastCalledWith(0);
  });
});

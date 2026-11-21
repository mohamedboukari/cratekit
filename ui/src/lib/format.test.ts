import { describe, expect, it } from "vitest";

import { filterText, formatDuration, formatTime } from "@/lib/format";

describe("display formatting", () => {
  it("formats track time", () => {
    expect(formatTime(0)).toBe("0:00");
    expect(formatTime(402.9)).toBe("6:42");
    expect(formatTime(-3)).toBe("0:00");
  });

  it("formats total durations", () => {
    expect(formatDuration(45 * 60)).toBe("45 min");
    expect(formatDuration(3 * 3600 + 41 * 60)).toBe("3 h 41 min");
  });

  it("describes the filter knob", () => {
    expect(filterText(0)).toBe("Off");
    expect(filterText(0.01)).toBe("Off");
    expect(filterText(-0.019)).toBe("Off");
    expect(filterText(0.02)).toBe("High-pass 2%");
    expect(filterText(-0.4)).toBe("Low-pass 40%");
    expect(filterText(0.25)).toBe("High-pass 25%");
  });
});

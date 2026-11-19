import { describe, expect, it } from "vitest";

import { filterText, formatDuration, formatTime } from "@/lib/format";
import { parseTrackDrop } from "@/lib/dnd";

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

describe("track drag payload", () => {
  it("accepts a list of integer ids only", () => {
    expect(parseTrackDrop("[1,2,3]")).toEqual([1, 2, 3]);
    expect(parseTrackDrop('[1,"x",2.5,4]')).toEqual([1, 4]);
    expect(parseTrackDrop('{"ids":[1]}')).toEqual([]);
    expect(parseTrackDrop("not json")).toEqual([]);
    expect(parseTrackDrop("")).toEqual([]);
  });
});

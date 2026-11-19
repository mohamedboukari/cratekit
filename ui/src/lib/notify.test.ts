import { toast } from "sonner";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { notifyError } from "@/lib/notify";

vi.mock("sonner", () => ({ toast: { error: vi.fn(), info: vi.fn() } }));

describe("notifyError", () => {
  beforeEach(() => vi.clearAllMocks());

  it("shows a feature from a later version as a calm note, not an error", () => {
    const result = notifyError(
      { code: "not_built", message: "playback_set_mode" },
      "Could not switch what you hear",
    );
    expect(result.code).toBe("not_built");
    expect(toast.info).toHaveBeenCalledWith("Not built yet", {
      description: "This part of Cratekit comes in a later version.",
    });
    expect(toast.error).not.toHaveBeenCalled();
  });

  it("shows a real failure as an error with the translated detail", () => {
    notifyError({ code: "decode", message: "bad frame" }, "Could not load the track");
    expect(toast.error).toHaveBeenCalledWith("Could not load the track", {
      description: "This audio file could not be decoded. (bad frame)",
    });
  });
});

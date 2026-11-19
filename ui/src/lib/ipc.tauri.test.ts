import { invoke } from "@tauri-apps/api/core";
import { describe, expect, it, vi } from "vitest";

import { call } from "@/lib/ipc.tauri";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

describe("call", () => {
  it("returns what the command returns", async () => {
    vi.mocked(invoke).mockResolvedValueOnce("0.1.0");
    await expect(call<string>("app_version")).resolves.toBe("0.1.0");
  });

  it("turns a missing Rust command into the not_built code", async () => {
    vi.mocked(invoke).mockRejectedValueOnce("Command playback_set_mode not found");
    await expect(call("playback_set_mode", { mode: "mastered" })).rejects.toEqual({
      code: "not_built",
      message: "playback_set_mode",
    });
  });

  it("passes real command errors through unchanged", async () => {
    const failure = { code: "decode", message: "bad frame" };
    vi.mocked(invoke).mockRejectedValueOnce(failure);
    await expect(call("playback_load", { id: 1 })).rejects.toBe(failure);
  });

  it("leaves other plain-text rejections alone", async () => {
    vi.mocked(invoke).mockRejectedValueOnce("Command app_version failed: busy");
    await expect(call("app_version")).rejects.toBe("Command app_version failed: busy");
  });
});

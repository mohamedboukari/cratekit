import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { App } from "@/App";

// Components only reach Rust through @/lib/ipc, so tests mock that one module.
vi.mock("@/lib/ipc", () => ({
  appVersion: vi.fn(() => Promise.resolve("0.1.0")),
}));

describe("App", () => {
  it("shows the empty state", () => {
    render(<App />);
    expect(screen.getByText("Drop tracks or a folder")).toBeInTheDocument();
  });

  it("shows the version reported by Rust", async () => {
    render(<App />);
    expect(await screen.findByText("v0.1.0")).toBeInTheDocument();
  });
});

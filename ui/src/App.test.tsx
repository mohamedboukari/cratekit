import { render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it } from "vitest";

import { App } from "@/App";
import { setApiForTests } from "@/lib/ipc";
import { createMockApi } from "@/mock/api";

describe("App", () => {
  beforeEach(() => {
    setApiForTests(createMockApi());
  });

  it("renders the shell with library data from the API", async () => {
    render(<App />);
    expect(
      await screen.findByRole("navigation", { name: "Library" }),
    ).toBeInTheDocument();
    expect(await screen.findByText("Friday club set")).toBeInTheDocument();
    expect(screen.getByRole("tablist", { name: "Tools" })).toBeInTheDocument();
  });

  it("shows the demo banner in mock mode and the version from the API", async () => {
    render(<App />);
    expect(await screen.findByRole("note")).toHaveTextContent("Demo with fake data");
    expect(await screen.findByText("v0.1.0-demo")).toBeInTheDocument();
  });
});

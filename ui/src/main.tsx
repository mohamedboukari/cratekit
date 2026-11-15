import { StrictMode } from "react";
import { createRoot } from "react-dom/client";

import { App } from "@/App";
import { initApi } from "@/lib/ipc";
import "@/index.css";

const root = document.getElementById("root");
if (!root) {
  throw new Error("Cratekit: #root element missing from index.html");
}

/** The API implementation (Tauri or mock) must be ready before any store runs. */
async function start(container: HTMLElement): Promise<void> {
  await initApi();
  createRoot(container).render(
    <StrictMode>
      <App />
    </StrictMode>,
  );
}

void start(root);

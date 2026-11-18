import { StrictMode, type JSX } from "react";
import { createRoot } from "react-dom/client";

import { App } from "@/App";
// Loads the translations before anything renders (English by default).
import "@/i18n";
import { SettingsApp } from "@/components/settings/SettingsApp";
import { initApi } from "@/lib/ipc";
import "@/index.css";

const root = document.getElementById("root");
if (!root) {
  throw new Error("Cratekit: #root element missing from index.html");
}

/** Rust opens the Settings window at `index.html#settings` (src-tauri/src/lib.rs). */
function Root(): JSX.Element {
  return window.location.hash === "#settings" ? <SettingsApp /> : <App />;
}

/** The API implementation (Tauri or mock) must be ready before any store runs. */
async function start(container: HTMLElement): Promise<void> {
  await initApi();
  createRoot(container).render(
    <StrictMode>
      <Root />
    </StrictMode>,
  );
}

void start(root);

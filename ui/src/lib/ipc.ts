/**
 * The only module that talks to Rust. Components and hooks import typed
 * functions from here and never call `invoke()` or `listen()` themselves.
 *
 * Every Rust command rejects with `{ code, message }` (AppError in
 * src-tauri/src/lib.rs) when it fails.
 */
import { invoke } from "@tauri-apps/api/core";

/** The Cratekit version reported by the Rust backend. */
export function appVersion(): Promise<string> {
  return invoke<string>("app_version");
}

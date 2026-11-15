import { useEffect, useEffectEvent } from "react";

import { type ActionId, isMacPlatform, matchShortcut } from "@/lib/shortcuts";

export type ShortcutHandlers = Partial<Record<ActionId, () => void>>;

/**
 * Dispatches keyboard shortcuts from the single map in lib/shortcuts.ts.
 * Mount once near the root. useEffectEvent always sees the latest handlers,
 * so passing a new object every render does not re-attach the listener.
 */
export function useShortcuts(handlers: ShortcutHandlers): void {
  /** Runs the handler for an action; returns false when none is registered. */
  const run = useEffectEvent((action: ActionId): boolean => {
    const handler = handlers[action];
    if (!handler) return false;
    handler();
    return true;
  });

  useEffect(() => {
    const isMac = isMacPlatform();
    const onKeyDown = (event: KeyboardEvent): void => {
      const action = matchShortcut(event, isMac);
      if (action && run(action)) event.preventDefault();
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, []);
}

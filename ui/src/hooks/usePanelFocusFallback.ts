import { useEffect, useRef } from "react";

/**
 * When a tool view swaps (Split -> progress -> mixer, not analyzed ->
 * analyzed), the focused button can unmount and focus drops to <body>.
 * This sends it to the tool panel instead, so keyboard users stay in place.
 */
export function usePanelFocusFallback(view: string): void {
  const previous = useRef(view);
  useEffect(() => {
    if (previous.current === view) return;
    previous.current = view;
    const active = document.activeElement;
    if (active === null || active === document.body)
      document.getElementById("tool-panel")?.focus();
  }, [view]);
}

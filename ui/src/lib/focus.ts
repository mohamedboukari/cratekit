import type { MouseEvent } from "react";

/**
 * Call from a button's onClick: after a mouse or touch click the button gives
 * focus back, so Space keeps meaning play/pause instead of clicking it again.
 * Keyboard activation (detail 0) keeps focus where it is.
 */
export function releasePointerFocus(event: MouseEvent<HTMLElement>): void {
  if (event.detail > 0) event.currentTarget.blur();
}

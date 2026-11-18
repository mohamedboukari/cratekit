import { toast } from "sonner";

import { i18n } from "@/i18n";
import { toAppError } from "@/lib/ipc";
import type { AppError } from "@/lib/types";

/** The translated detail for a Rust error code we know, or null for unknown codes. */
function codeDetail(code: string): string | null {
  switch (code) {
    case "io":
      return i18n.t("errors.codes.io");
    case "not_found":
      return i18n.t("errors.codes.not_found");
    case "model_missing":
      return i18n.t("errors.codes.model_missing");
    case "window":
      return i18n.t("errors.codes.window");
    default:
      return null;
  }
}

/**
 * Shows a command failure as a toast and returns the normalized error. The
 * title is the (translated) context; the detail is the translated sentence for
 * a known error code followed by Rust's own message, or Rust's message alone.
 */
export function notifyError(error: unknown, context?: string): AppError {
  const appError = toAppError(error);
  const detail = codeDetail(appError.code);
  // Keep Rust's specific message (a file path, an OS reason) after the
  // translated sentence: it is what a user needs to act on or report.
  const description =
    detail && appError.message && appError.message !== detail
      ? `${detail} (${appError.message})`
      : (detail ?? appError.message);
  toast.error(context ?? i18n.t("common.somethingWentWrong"), { description });
  return appError;
}

/** Tells the user how an import went (picker or drop), including when nothing was new. */
export function announceImport(added: number): void {
  if (added > 0) toast.success(i18n.t("common.import.added", { count: added }));
  else toast.info(i18n.t("common.import.none"));
}

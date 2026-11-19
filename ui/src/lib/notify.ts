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
    case "unsupported_format":
      return i18n.t("errors.codes.unsupported_format");
    case "decode":
      return i18n.t("errors.codes.decode");
    case "no_audio":
      return i18n.t("errors.codes.no_audio");
    case "audio_processing":
      return i18n.t("errors.codes.audio_processing");
    case "database":
      return i18n.t("errors.codes.database");
    case "database_too_new":
      return i18n.t("errors.codes.database_too_new");
    case "no_audio_device":
      return i18n.t("errors.codes.no_audio_device");
    case "audio_device":
      return i18n.t("errors.codes.audio_device");
    case "invalid_setting":
      return i18n.t("errors.codes.invalid_setting");
    case "invalid_name":
      return i18n.t("errors.codes.invalid_name");
    case "download":
      return i18n.t("errors.codes.download");
    case "checksum":
      return i18n.t("errors.codes.checksum");
    case "busy":
      return i18n.t("errors.codes.busy");
    case "stems":
      return i18n.t("errors.codes.stems");
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
  // A feature planned for a later version is not a failure: say so calmly.
  if (appError.code === "not_built") {
    toast.info(i18n.t("errors.notBuilt.title"), {
      description: i18n.t("errors.notBuilt.detail"),
    });
    return appError;
  }
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

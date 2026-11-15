import { toast } from "sonner";

import { toAppError } from "@/lib/ipc";
import type { AppError } from "@/lib/types";

/** Shows a command failure as a toast and returns the normalized error. */
export function notifyError(error: unknown, context?: string): AppError {
  const appError = toAppError(error);
  toast.error(context ?? "Something went wrong", { description: appError.message });
  return appError;
}

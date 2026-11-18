import type { Translation } from "@/i18n/en";
import { browser } from "@/i18n/fr/browser";
import { common } from "@/i18n/fr/common";
import { deck } from "@/i18n/fr/deck";
import { dialogs } from "@/i18n/fr/dialogs";
import { errors } from "@/i18n/fr/errors";
import { layout } from "@/i18n/fr/layout";
import { settings } from "@/i18n/fr/settings";
import { shortcuts } from "@/i18n/fr/shortcuts";
import { tools } from "@/i18n/fr/tools";

/** French. A missing or extra key here is a type error. */
export const fr: Translation = {
  common,
  deck,
  layout,
  browser,
  dialogs,
  tools,
  settings,
  shortcuts,
  errors,
};

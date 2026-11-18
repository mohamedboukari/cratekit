import { browser } from "@/i18n/en/browser";
import { common } from "@/i18n/en/common";
import { deck } from "@/i18n/en/deck";
import { dialogs } from "@/i18n/en/dialogs";
import { errors } from "@/i18n/en/errors";
import { layout } from "@/i18n/en/layout";
import { settings } from "@/i18n/en/settings";
import { shortcuts } from "@/i18n/en/shortcuts";
import { tools } from "@/i18n/en/tools";

/**
 * English: the source language and the default for every user. Each area of
 * the UI owns one file in this folder; `fr/` mirrors it key for key.
 */
export const en = {
  common,
  deck,
  layout,
  browser,
  dialogs,
  tools,
  settings,
  shortcuts,
  errors,
} as const;

/** Same shape as the English source, with any text as values. */
type Shape<T> = { [K in keyof T]: T[K] extends string ? string : Shape<T[K]> };

/** What every other language must provide: every English key, nothing more. */
export type Translation = Shape<typeof en>;

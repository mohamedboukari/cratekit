import type { en } from "@/i18n/en";

// Typed keys: t("deck.cue") autocompletes, and a key that does not exist in
// the English source is a type error.
declare module "i18next" {
  interface CustomTypeOptions {
    defaultNS: "translation";
    resources: { translation: typeof en };
  }
}

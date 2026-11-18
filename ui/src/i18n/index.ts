/**
 * UI languages (#80). English is the default for every user, whatever the
 * system language; other languages are chosen in Settings. Strings live in
 * `en/` (the source) and one folder per language, one file per UI area.
 */
import { createInstance } from "i18next";
import { initReactI18next } from "react-i18next";

import { en } from "@/i18n/en";
import { fr } from "@/i18n/fr";
import { api } from "@/lib/ipc";
import type { MenuLabels, UiLanguage } from "@/lib/types";

/** Languages offered in Settings, each named in its own language. */
export const LANGUAGES: ReadonlyArray<{ code: UiLanguage; name: string }> = [
  { code: "en", name: "English" },
  { code: "fr", name: "Français" },
];

/** Cratekit's i18next instance (react-i18next picks it up through `initReactI18next`). */
const i18n = createInstance();

void i18n.use(initReactI18next).init({
  resources: { en: { translation: en }, fr: { translation: fr } },
  lng: "en",
  fallbackLng: "en",
  // React already escapes text.
  interpolation: { escapeValue: false },
  // Resources are bundled, so there is nothing to wait for.
  initAsync: false,
});

/** Every native menu label and the Settings window title, in the current language. */
function menuLabels(): MenuLabels {
  return {
    settings: i18n.t("common.menu.settings"),
    settingsWindowTitle: i18n.t("common.menu.settingsWindowTitle"),
    file: i18n.t("common.menu.file"),
    edit: i18n.t("common.menu.edit"),
    window: i18n.t("common.menu.window"),
    undo: i18n.t("common.menu.undo"),
    redo: i18n.t("common.menu.redo"),
    cut: i18n.t("common.menu.cut"),
    copy: i18n.t("common.menu.copy"),
    paste: i18n.t("common.menu.paste"),
    selectAll: i18n.t("common.menu.selectAll"),
    minimize: i18n.t("common.menu.minimize"),
    closeWindow: i18n.t("common.menu.closeWindow"),
    quit: i18n.t("common.menu.quit"),
    about: i18n.t("common.menu.about"),
    services: i18n.t("common.menu.services"),
    hide: i18n.t("common.menu.hide"),
    hideOthers: i18n.t("common.menu.hideOthers"),
    showAll: i18n.t("common.menu.showAll"),
  };
}

/**
 * Switches the UI language: the text everywhere (react-i18next re-renders
 * what uses it), the page's `lang` attribute (screen readers, hyphenation)
 * and the native menu labels built by Rust.
 */
export function applyLanguage(language: UiLanguage): void {
  void i18n.changeLanguage(language);
  document.documentElement.lang = language;
  api()
    .appSetMenuLabels(menuLabels())
    .catch(() => {
      // The menu keeps its previous labels; nothing else depends on it.
    });
}

export { i18n };

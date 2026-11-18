import { afterEach, describe, expect, it } from "vitest";

import { applyLanguage, i18n, LANGUAGES } from "@/i18n";
import { en } from "@/i18n/en";
import { fr } from "@/i18n/fr";
import { setApiForTests } from "@/lib/ipc";
import type { MenuLabels } from "@/lib/types";
import { notifyError } from "@/lib/notify";
import { createMockApi } from "@/mock/api";

/** Every leaf of a nested translation object as "path: value". */
function leaves(value: unknown, path = ""): Array<[string, unknown]> {
  if (typeof value !== "object" || value === null) return [[path, value]];
  return Object.entries(value).flatMap(([key, child]) =>
    leaves(child, path ? `${path}.${key}` : key),
  );
}

describe("translations", () => {
  afterEach(() => {
    void i18n.changeLanguage("en");
  });

  it("French has exactly the English keys, each with real text", () => {
    const english = leaves(en);
    const french = new Map(leaves(fr));
    expect(english.length).toBeGreaterThan(0);
    expect([...french.keys()].toSorted()).toEqual(english.map(([k]) => k).toSorted());
    for (const [key, value] of french) {
      expect(typeof value, key).toBe("string");
      expect(String(value).trim(), key).not.toBe("");
    }
  });

  it("never uses em or en dashes", () => {
    for (const [key, value] of [...leaves(en), ...leaves(fr)]) {
      expect(String(value), key).not.toMatch(/[\u2013\u2014]/);
    }
  });

  it("starts in English for every user and lists each language in its own name", () => {
    expect(i18n.language).toBe("en");
    expect(LANGUAGES.map((l) => l.name)).toEqual(["English", "Français"]);
  });

  it("uses each language's plural rules", () => {
    expect(i18n.t("common.import.added", { count: 1 })).toBe("Added 1 track");
    expect(i18n.t("common.import.added", { count: 12 })).toBe("Added 12 tracks");
    void i18n.changeLanguage("fr");
    expect(i18n.t("common.import.added", { count: 1 })).toBe("1 morceau ajouté");
    expect(i18n.t("common.import.added", { count: 12 })).toBe("12 morceaux ajoutés");
  });

  it("translates known Rust error codes and keeps Rust's message for unknown ones", () => {
    void i18n.changeLanguage("fr");
    const known = notifyError({ code: "not_found", message: "No track 9" });
    expect(known.code).toBe("not_found");
    expect(i18n.t("errors.codes.not_found")).toBe("Cet élément n'existe plus.");
    const unknown = notifyError({ code: "brand_new", message: "Something specific" });
    expect(unknown.message).toBe("Something specific");
  });

  it("relabels the native menu when the language changes", async () => {
    const sent: MenuLabels[] = [];
    const mock = createMockApi();
    setApiForTests({
      ...mock,
      appSetMenuLabels: (labels) => {
        sent.push(labels);
        return mock.appSetMenuLabels(labels);
      },
    });
    applyLanguage("fr");
    expect(document.documentElement.lang).toBe("fr");
    await Promise.resolve();
    expect(sent.at(-1)).toMatchObject({
      settings: "Réglages...",
      file: "Fichier",
      edit: "Édition",
      window: "Fenêtre",
      copy: "Copier",
      quit: "Quitter",
      settingsWindowTitle: "Réglages de Cratekit",
    });
    applyLanguage("en");
    expect(sent.at(-1)?.settings).toBe("Settings...");
  });
});

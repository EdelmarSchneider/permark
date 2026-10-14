import { afterEach, describe, expect, it } from "vitest";
import { en } from "./en";
import { formatNumber, keys, language, MESSAGES, pluralCategory, t, tn } from "./index";
import {
  LANGUAGE_NAMES,
  LANGUAGE_PREFERENCES,
  LOCALES,
  matchLocale,
  parsePreference,
  resolveLocale,
  systemLocale,
  type Locale,
} from "./locales";

afterEach(() => {
  language.preference = "en";
});

describe("t", () => {
  it("returns the English string for a key", () => {
    expect(t("app.name")).toBe("TrailMark");
  });

  it("fills placeholders", () => {
    expect(t("welcome.openFailed", { path: "F:/x", message: "access denied" })).toBe(
      "Can't open F:/x: access denied",
    );
  });

  it("leaves unknown placeholders visible", () => {
    expect(t("doc.saved", {})).toBe("saved {time} ago");
  });

  it("follows the language at once, with no other state to refresh", () => {
    language.preference = "de";
    expect(t("common.cancel")).toBe("Abbrechen");
    language.preference = "pt-BR";
    expect(t("common.cancel")).toBe("Cancelar");
    language.preference = "es";
    expect(t("common.cancel")).toBe("Cancelar");
    language.preference = "en";
    expect(t("common.cancel")).toBe("Cancel");
  });
});

describe("tn", () => {
  it("picks the singular and plural forms", () => {
    expect(tn("feed.saves", 1)).toBe("1 save");
    expect(tn("feed.saves", 3)).toBe("3 saves");
    expect(tn("feed.saves", 0)).toBe("0 saves");
  });

  it("formats large counts and extra parameters", () => {
    expect(tn("toast.docsChanged", 1200)).toBe("1,200 docs changed");
    expect(tn("doc.removedBlocks", 1, { time: "10:42" })).toBe("1 block removed · 10:42");
  });

  it("writes the count the way the language does", () => {
    language.preference = "de";
    expect(tn("toast.docsChanged", 1200)).toBe("1.200 Dokumente geändert");
    expect(tn("toast.docsChanged", 1)).toBe("1 Dokument geändert");
    language.preference = "pt-BR";
    expect(tn("toast.docsChanged", 1200)).toBe("1.200 docs alterados");
    language.preference = "es";
    expect(tn("toast.docsChanged", 3)).toBe("3 docs cambiaron");
  });

  it("writes zero as a plural in Brazilian Portuguese and by CLDR in the other languages", () => {
    language.preference = "pt-BR";
    expect(tn("feed.saves", 0)).toBe("0 salvamentos");
    expect(tn("feed.saves", 1)).toBe("1 salvamento");
    expect(tn("fileOps.files", 0)).toBe("0 arquivos");
    language.preference = "es";
    expect(tn("fileOps.files", 0)).toBe("0 archivos");
    language.preference = "de";
    expect(tn("fileOps.files", 0)).toBe("0 Dateien");
    language.preference = "en";
    expect(tn("fileOps.files", 0)).toBe("0 files");
  });

  it("uses each language's own plural rules", () => {
    // CLDR counts 0 as singular in Portuguese, but Brazilian usage writes "0 arquivos": zero is plural there
    language.preference = "pt-BR";
    expect(pluralCategory(0)).toBe("other");
    expect(pluralCategory(1)).toBe("one");
    expect(pluralCategory(2)).toBe("other");
    language.preference = "de";
    expect(pluralCategory(0)).toBe("other");
    expect(pluralCategory(1)).toBe("one");
    language.preference = "es";
    expect(pluralCategory(1)).toBe("one");
    expect(pluralCategory(2)).toBe("other");
  });

  it("reads a category a language has no key for (Spanish and Portuguese 'many') as 'other'", () => {
    language.preference = "es";
    expect(pluralCategory(1_000_000)).toBe("many");
    expect(tn("toast.docsChanged", 1_000_000)).toBe("1.000.000 docs cambiaron");
  });
});

describe("formatNumber", () => {
  it("groups digits by the language of the interface", () => {
    expect(formatNumber(12345)).toBe("12,345");
    language.preference = "de";
    expect(formatNumber(12345)).toBe("12.345");
    language.preference = "pt-BR";
    expect(formatNumber(12345)).toBe("12.345");
  });
});

describe("keys", () => {
  it("writes the key names of a combo as the language's keyboards do", () => {
    expect(keys("Ctrl+Shift+O")).toBe("Ctrl+Shift+O");
    language.preference = "de";
    expect(keys("Ctrl+Shift+O")).toBe("Strg+Umschalt+O");
    expect(keys("Ctrl+=")).toBe("Strg+=");
    expect(keys("Del")).toBe("Entf");
    language.preference = "es";
    expect(keys("Ctrl+Shift+O")).toBe("Ctrl+Mayús+O");
    language.preference = "pt-BR";
    expect(keys("Ctrl+Shift+O")).toBe("Ctrl+Shift+O");
  });
});

describe("the Semantic teaser", () => {
  it("names the release it is planned for (v0.3), everywhere it shows, in every language", () => {
    for (const locale of LOCALES) {
      language.preference = locale;
      for (const key of ["search.soon", "search.semanticLabel", "search.semanticHint"] as const) {
        expect(t(key), `${locale} ${key}`).toContain("v0.3");
        expect(t(key), `${locale} ${key}`).not.toContain("v0.2");
      }
    }
  });
});

// ---- choosing the language ---------------------------------------------------------------------------------------

describe("matchLocale", () => {
  it("reads a tag by its language, whatever the region", () => {
    expect(matchLocale("en-GB")).toBe("en");
    expect(matchLocale("pt-BR")).toBe("pt-BR");
    expect(matchLocale("pt-PT")).toBe("pt-BR");
    expect(matchLocale("pt")).toBe("pt-BR");
    expect(matchLocale("es-MX")).toBe("es");
    expect(matchLocale("es_AR")).toBe("es");
    expect(matchLocale("DE-at")).toBe("de");
    expect(matchLocale("fr-FR")).toBeNull();
    expect(matchLocale("")).toBeNull();
  });
});

describe("systemLocale and resolveLocale", () => {
  it("takes the first language of the system that the app speaks", () => {
    expect(systemLocale(["de-DE", "en-US"])).toBe("de");
    expect(systemLocale(["fr-FR", "es-419", "en"])).toBe("es");
    expect(systemLocale(["pt-BR"])).toBe("pt-BR");
  });

  it("falls back to English", () => {
    expect(systemLocale(["fr-FR", "ja"])).toBe("en");
    expect(systemLocale([])).toBe("en");
  });

  it("lets a chosen language win over the system's", () => {
    expect(resolveLocale("de", ["pt-BR"])).toBe("de");
    expect(resolveLocale("system", ["pt-BR"])).toBe("pt-BR");
    expect(resolveLocale("system", ["zz"])).toBe("en");
  });

  it("reads anything unknown as the system default", () => {
    expect(parsePreference("de")).toBe("de");
    expect(parsePreference("system")).toBe("system");
    for (const bad of ["fr", "PT-br", "", null, undefined, 7, {}]) {
      expect(parsePreference(bad)).toBe("system");
    }
  });

  it("names each language in itself", () => {
    expect(LANGUAGE_NAMES).toEqual({
      en: "English",
      "pt-BR": "Português (Brasil)",
      es: "Español",
      de: "Deutsch",
    });
    expect(LANGUAGE_PREFERENCES).toEqual(["system", "en", "pt-BR", "es", "de"]);
  });
});

// ---- the tables --------------------------------------------------------------------------------------------------

const english = en as Record<string, string>;
const KEYS = Object.keys(english);
const OTHER: Locale[] = LOCALES.filter((locale) => locale !== "en");

/** The `{name}` placeholders of a string, sorted. */
function placeholders(text: string): string[] {
  return [...text.matchAll(/\{(\w+)\}/g)].map((m) => m[1]).sort();
}

describe.each(OTHER)("the %s table", (locale) => {
  const table = MESSAGES[locale] as Record<string, string>;

  it("has exactly the keys of English: none missing, none extra", () => {
    expect(KEYS.filter((k) => !(k in table))).toEqual([]);
    expect(Object.keys(table).filter((k) => !(k in english))).toEqual([]);
  });

  it("uses the same placeholders as English for every key", () => {
    const wrong = KEYS.filter(
      (k) =>
        JSON.stringify(placeholders(table[k] ?? "")) !== JSON.stringify(placeholders(english[k])),
    ).map((k) => `${k}: ${table[k]}`);
    expect(wrong).toEqual([]);
  });

  it("has no empty string where English has text", () => {
    expect(KEYS.filter((k) => english[k] !== "" && (table[k] ?? "").trim() === "")).toEqual([]);
  });

  it("keeps the product name and leaves no long sentence in English", () => {
    expect(table["app.name"]).toBe("TrailMark");
    // identical to English is fine for names, versions and extensions; a long sentence left in English is a gap
    expect(KEYS.filter((k) => english[k].length > 40 && table[k] === english[k])).toEqual([]);
  });

  it("names the same key in every string that names a shortcut", () => {
    // the key after the modifiers is the same in every language (only Ctrl, Shift and Enter get local names)
    const modifiers =
      /(?:Ctrl|Strg|Alt)\+(?:(?:Shift|Umschalt|Mayús|Alt|Ctrl|Strg)\+)*([^\s+)])(?![A-Za-zÀ-ÿ])/g;
    const wrong: string[] = [];
    for (const k of KEYS) {
      const expected = [...english[k].matchAll(modifiers)].map((m) => m[1]);
      if (expected.length === 0) continue;
      const found = [...(table[k] ?? "").matchAll(modifiers)].map((m) => m[1]);
      if (JSON.stringify(expected) !== JSON.stringify(found)) wrong.push(`${k}: ${table[k]}`);
    }
    expect(wrong).toEqual([]);
  });

  it("carries {n} in the plural forms English carries it in", () => {
    const wrong = KEYS.filter((k) => /\.(one|other)$/.test(k)).filter(
      (k) => placeholders(table[k]).includes("n") !== placeholders(english[k]).includes("n"),
    );
    expect(wrong).toEqual([]);
  });
});

describe("the English table", () => {
  it("only leaves a string empty for a state with nothing to say", () => {
    expect(KEYS.filter((k) => english[k] === "")).toEqual([
      "repo.banner.clean",
      "status.chip.clean",
    ]);
  });

  it("has a .one and a .other for every plural key", () => {
    for (const k of KEYS.filter((key) => key.endsWith(".one"))) {
      expect(KEYS, k).toContain(`${k.slice(0, -4)}.other`);
    }
    // three keys that end in "other" name a state, not a plural
    const unpaired = KEYS.filter(
      (key) => key.endsWith(".other") && !KEYS.includes(`${key.slice(0, -6)}.one`),
    );
    expect(unpaired).toEqual(["repo.banner.other", "status.chip.other", "edit.locked.other"]);
  });
});

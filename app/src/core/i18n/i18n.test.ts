import { readdirSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { afterEach, describe, expect, it } from "vitest";
import { fill, language, t, tn } from "./index";
import {
  LOCALES,
  matchLocale,
  parsePreference,
  resolveLocale,
  systemLocale,
  type Locale,
} from "./locales";
import en from "./locales/en";
import { discoveredLocales, loadTable, localeOfPath } from "./table";

afterEach(async () => {
  await language.choose("en", []);
});

// ---- spec ---------------------------------------------------------------------------------------------------------

describe("t", () => {
  it("returns the English string for a key", () => {
    expect(t("app.name")).toBe("PerMark");
  });

  it("fills placeholders", () => {
    expect(t("status.version", { version: "1.2.3" })).toBe("Version 1.2.3");
  });

  it("follows the chosen language at once, after its table loads", async () => {
    await language.choose("de", []);
    expect(t("app.loading")).toBe("Wird geladen");
    await language.choose("pt-BR", []);
    expect(t("status.version", { version: "1.2.3" })).toBe("Versão 1.2.3");
    await language.choose("zh-CN", []);
    expect(t("status.version", { version: "1.2.3" })).toBe("版本 1.2.3");
    await language.choose("en", []);
    expect(t("app.loading")).toBe("Loading");
  });

  it("follows the system language when the preference is 'system'", async () => {
    await language.choose("system", ["it-IT", "en-US"]);
    expect(language.locale).toBe("it");
    expect(t("app.loading")).toBe("Caricamento");
  });
});

describe("tn", () => {
  it("picks the singular or plural form of the active language and fills the count", async () => {
    expect(tn("fileOps.files", 1)).toBe("1 file");
    expect(tn("fileOps.files", 0)).toBe("0 files");
    expect(tn("fileOps.files", 3)).toBe("3 files");
    expect(tn("fileOps.updateLinks", 2, { files: tn("fileOps.files", 1) })).toBe(
      "Update 2 links in 1 file?",
    );
    await language.choose("pt-BR", []);
    expect(tn("fileOps.files", 1)).toMatch(/^1 /);
    expect(tn("fileOps.files", 2)).toMatch(/^2 /);
    await language.choose("zh-CN", []);
    expect(tn("fileOps.files", 5)).toContain("5");
  });
});

describe("matchLocale, systemLocale, resolveLocale", () => {
  it("reads a tag by its primary subtag, whatever the region", () => {
    expect(matchLocale("en-GB")).toBe("en");
    expect(matchLocale("pt-PT")).toBe("pt-BR");
    expect(matchLocale("es_AR")).toBe("es");
    expect(matchLocale("DE-at")).toBe("de");
    expect(matchLocale("zh-Hant-TW")).toBe("zh-CN");
    expect(matchLocale("it-CH")).toBe("it");
  });

  it("takes the first system language the app speaks, else English", () => {
    expect(systemLocale(["fr-FR", "es-419", "en"])).toBe("es");
    expect(systemLocale(["fr-FR", "ja"])).toBe("en");
    expect(systemLocale([])).toBe("en");
  });

  it("lets a chosen language win over the system's", () => {
    expect(resolveLocale("de", ["pt-BR"])).toBe("de");
    expect(resolveLocale("system", ["pt-BR"])).toBe("pt-BR");
  });
});

// ---- adversarial ----------------------------------------------------------------------------------------------------

describe("malformed input", () => {
  it("rejects tags that only start like a supported language", () => {
    for (const tag of ["", " ", "-", "zha", "zho-CN", "ita-IT", "itx", "english", "ptBR"]) {
      expect(matchLocale(tag), JSON.stringify(tag)).toBeNull();
    }
  });

  it("reads any unknown stored preference as 'system'", () => {
    for (const bad of [
      "fr",
      "PT-br",
      "zh",
      "zh-cn",
      "IT",
      "",
      " de",
      null,
      undefined,
      7,
      {},
      [],
      ["de"],
    ]) {
      expect(parsePreference(bad), JSON.stringify(bad)).toBe("system");
    }
    for (const good of [...LOCALES, "system"]) {
      expect(parsePreference(good)).toBe(good);
    }
  });

  it("leaves unknown or missing placeholders visible", () => {
    expect(t("status.version")).toBe("Version {version}");
    expect(t("status.version", {})).toBe("Version {version}");
    expect(fill("{a} {b}", { a: 1 })).toBe("1 {b}");
  });

  it("never re-expands placeholder syntax found inside a value", () => {
    expect(fill("{a}-{b}", { a: "{b}", b: "x" })).toBe("{b}-x");
    expect(fill("{a}", { a: "$&$1" })).toBe("$&$1");
  });

  it("does not read prototype properties as parameters", () => {
    expect(fill("{constructor} {toString}", {})).toBe("{constructor} {toString}");
  });

  it("survives rapid language switches: the last choice wins", async () => {
    const picks: Locale[] = ["de", "es", "zh-CN", "it", "pt-BR"];
    await Promise.all(picks.map((locale) => language.choose(locale, [])));
    expect(language.locale).toBe("pt-BR");
    expect(t("app.loading")).toBe("Carregando");
  });
});

// ---- tables: parity -----------------------------------------------------------------------------------------------

const KEYS = Object.keys(en);
const english: Record<string, string> = en;

function placeholders(text: string): string[] {
  return [...text.matchAll(/\{(\w+)\}/g)].map((m) => m[1] ?? "").sort();
}

describe("locale directory", () => {
  const dir = fileURLToPath(new URL("./locales/", import.meta.url));
  const files = readdirSync(dir)
    .filter((name) => name.endsWith(".ts"))
    .map((name) => localeOfPath(name));

  it("holds exactly one table per supported locale", () => {
    expect([...files].sort()).toEqual([...LOCALES].sort());
  });

  it("is what the loader discovers (English bundled, others on demand)", () => {
    expect(discoveredLocales().sort()).toEqual(LOCALES.filter((l) => l !== "en").sort());
  });
});

describe.each(LOCALES.filter((locale) => locale !== "en"))("the %s table", (locale) => {
  it("loads, with exactly the keys of English", async () => {
    const result = await loadTable(locale);
    if (result.status !== "loaded") throw new Error(`${locale}: ${result.status}`);
    const table: Record<string, string> = result.table;
    expect(KEYS.filter((k) => !(k in table))).toEqual([]);
    expect(Object.keys(table).filter((k) => !(k in english))).toEqual([]);
  });

  it("uses the same placeholders as English, and no empty string", async () => {
    const result = await loadTable(locale);
    if (result.status !== "loaded") throw new Error(`${locale}: ${result.status}`);
    const table: Record<string, string> = result.table;
    for (const key of KEYS) {
      const value = table[key] ?? "";
      expect(placeholders(value), `${locale} ${key}`).toEqual(placeholders(english[key] ?? ""));
      expect(value.trim(), `${locale} ${key}`).not.toBe("");
    }
  });

  it("keeps the product name", async () => {
    const result = await loadTable(locale);
    if (result.status !== "loaded") throw new Error(`${locale}: ${result.status}`);
    expect(result.table["app.name"]).toBe("PerMark");
  });
});

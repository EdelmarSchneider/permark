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
    await language.choose("ja", []);
    expect(t("status.version", { version: "1.2.3" })).toBe("バージョン 1.2.3");
    await language.choose("ko", []);
    expect(t("status.version", { version: "1.2.3" })).toBe("버전 1.2.3");
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
    await language.choose("ja", []);
    expect(tn("fileOps.files", 1)).toBe("1 個のファイル");
    expect(tn("fileOps.files", 5)).toBe("5 個のファイル");
    await language.choose("ko", []);
    expect(tn("fileOps.files", 1)).toBe("파일 1개");
    expect(tn("fileOps.files", 5)).toBe("파일 5개");
    await language.choose("fr", []);
    expect(tn("fileOps.files", 0)).toBe("0 fichier");
    expect(tn("fileOps.files", 2)).toBe("2 fichiers");
    await language.choose("pt-PT", []);
    expect(tn("fileOps.files", 0)).toBe("0 ficheiros");
    expect(tn("fileOps.files", 1)).toBe("1 ficheiro");
  });

  it("reads a third form where the language has one (Romanian .few)", async () => {
    await language.choose("ro", []);
    expect(tn("fileOps.files", 1)).toBe("1 fișier");
    expect(tn("fileOps.files", 0)).toBe("0 fișiere");
    expect(tn("fileOps.files", 19)).toBe("19 fișiere");
    expect(tn("fileOps.files", 20)).toBe("20 de fișiere");
    expect(tn("fileOps.files", 101)).toBe("101 fișiere");
    expect(tn("fileOps.files", 120)).toBe("120 de fișiere");
  });
});

describe("matchLocale, systemLocale, resolveLocale", () => {
  it("reads a tag by its primary subtag, whatever the region", () => {
    expect(matchLocale("en-GB")).toBe("en");
    expect(matchLocale("pt-PT")).toBe("pt-PT");
    expect(matchLocale("es_AR")).toBe("es");
    expect(matchLocale("DE-at")).toBe("de");
    expect(matchLocale("zh-Hant-TW")).toBe("zh-CN");
    expect(matchLocale("it-CH")).toBe("it");
    for (const tag of ["ja", "ja-JP", "ja_JP", "JA-jp"]) expect(matchLocale(tag), tag).toBe("ja");
    for (const tag of ["ko", "ko-KR", "ko_KR", "KO-kr"]) {
      expect(matchLocale(tag), tag).toBe("ko");
    }
    for (const tag of ["fr", "fr-FR", "fr_CA", "FR-be"]) expect(matchLocale(tag), tag).toBe("fr");
    for (const tag of ["ca", "ca-ES", "ca-ES-valencia", "ca_AD"]) {
      expect(matchLocale(tag), tag).toBe("ca");
    }
    for (const tag of ["gl", "gl-ES", "GL_es"]) expect(matchLocale(tag), tag).toBe("gl");
    for (const tag of ["ro", "ro-RO", "ro_MD", "RO-ro"]) expect(matchLocale(tag), tag).toBe("ro");
  });

  it("reads Portuguese by region: Brazil (and a bare pt) or Portugal's norm", () => {
    for (const tag of ["pt", "pt-BR", "pt_br", "PT-BR", "pt-Latn", "pt-Latn-BR"]) {
      expect(matchLocale(tag), tag).toBe("pt-BR");
    }
    for (const tag of ["pt-PT", "pt_PT", "pt-AO", "pt-MZ", "pt-CV", "pt-Latn-PT", "pt-150"]) {
      expect(matchLocale(tag), tag).toBe("pt-PT");
    }
  });

  it("takes the first system language the app speaks, else English", () => {
    expect(systemLocale(["sv-SE", "es-419", "en"])).toBe("es");
    expect(systemLocale(["sv-SE", "nl"])).toBe("en");
    expect(systemLocale(["sv-SE", "ja-JP", "en"])).toBe("ja");
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
    const tags = ["", " ", "-", "zha", "zho-CN", "ita-IT", "itx", "kor", "kok", "english", "ptBR"];
    for (const tag of tags) {
      expect(matchLocale(tag), JSON.stringify(tag)).toBeNull();
    }
    for (const tag of ["jpn", "jav", "jp", "jpn-JP"]) {
      expect(matchLocale(tag), JSON.stringify(tag)).toBeNull();
    }
    for (const tag of ["fra", "frr", "cat", "glg", "gla", "ron", "rom", "rum", "por", "por-PT"]) {
      expect(matchLocale(tag), JSON.stringify(tag)).toBeNull();
    }
  });

  it("reads any unknown stored preference as 'system'", () => {
    for (const bad of [
      "fr-FR",
      "PT-br",
      "pt-pt",
      "pt",
      "zh",
      "zh-cn",
      "IT",
      "ja-JP",
      "JA",
      "KO",
      "ko-KR",
      "RO",
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
    const picks: Locale[] = ["de", "es", "zh-CN", "it", "ja", "ko", "fr", "pt-PT", "pt-BR"];
    await Promise.all(picks.map((locale) => language.choose(locale, [])));
    expect(language.locale).toBe("pt-BR");
    expect(t("app.loading")).toBe("Carregando");
  });
});

// ---- tables: parity -----------------------------------------------------------------------------------------------

const KEYS = Object.keys(en);
const english: Record<string, string> = en;
/** `<base>.few` -> `<base>.other`, for each English `.one` key: the third plural form a table may add. */
const FEW = new Map(
  KEYS.filter((key) => key.endsWith(".one")).map((key) => {
    const base = key.slice(0, -".one".length);
    return [`${base}.few`, `${base}.other`];
  }),
);

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
    expect(Object.keys(table).filter((k) => !(k in english) && !FEW.has(k))).toEqual([]);
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
    for (const [key, other] of FEW) {
      const value = table[key];
      if (value === undefined) continue;
      expect(placeholders(value), `${locale} ${key}`).toEqual(placeholders(english[other] ?? ""));
      expect(value.trim(), `${locale} ${key}`).not.toBe("");
    }
  });

  it("keeps the product name", async () => {
    const result = await loadTable(locale);
    if (result.status !== "loaded") throw new Error(`${locale}: ${result.status}`);
    expect(result.table["app.name"]).toBe("PerMark");
  });
});

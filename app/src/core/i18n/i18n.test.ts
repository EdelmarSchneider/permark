import { readdirSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { afterEach, describe, expect, it, vi } from "vitest";
import { fill, formatLocale, language, sizeText, t, tn } from "./index";
import type { FewForms, Messages } from "./messages";
import {
  formatTag,
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

  it("says Everything includes files ignored by git and hidden files", () => {
    expect(t("filter.everythingHint")).toBe(
      "All file types. Everything includes files ignored by git and hidden files. The switches below can turn either off.",
    );
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

  // Russian counts in three forms (one: 1, 21; few: 2-4, 22-24; many: 0, 5-20, 25-30): many shares `.other`
  it("reads Russian's one, few and many forms, many from .other", async () => {
    const before = language.tables;
    const ru: Messages & FewForms = {
      ...en,
      "fileOps.files.one": "{n} файл",
      "fileOps.files.few": "{n} файла",
      "fileOps.files.other": "{n} файлов",
    };
    language.tables = { ...before, ru };
    try {
      await language.choose("ru", []);
      const said = [1, 2, 5, 11, 21, 22, 25].map((n) => tn("fileOps.files", n));
      expect(said).toEqual([
        "1 файл",
        "2 файла",
        "5 файлов",
        "11 файлов",
        "21 файл",
        "22 файла",
        "25 файлов",
      ]);
    } finally {
      language.tables = before;
    }
  });

  // Indonesian, Japanese, Korean, Chinese and Vietnamese count with one form only ("other"): their `.one` text still
  // says exactly one (a name, "this file"), so a count of 1 reads it
  it.each([
    ["en", ["other 0", "one 1", "other 2", "other 5", "other 21", "other 101"]],
    ["fr", ["one 0", "one 1", "other 2", "other 5", "other 21", "other 101"]],
    ["ru", ["other 0", "one 1", "few 2", "other 5", "one 21", "one 101"]],
    ["ro", ["few 0", "one 1", "few 2", "few 5", "other 21", "few 101"]],
    ["id", ["other 0", "one 1", "other 2", "other 5", "other 21", "other 101"]],
    ["ja", ["other 0", "one 1", "other 2", "other 5", "other 21", "other 101"]],
    ["ko", ["other 0", "one 1", "other 2", "other 5", "other 21", "other 101"]],
    ["zh-CN", ["other 0", "one 1", "other 2", "other 5", "other 21", "other 101"]],
    ["zh-TW", ["other 0", "one 1", "other 2", "other 5", "other 21", "other 101"]],
    ["vi", ["other 0", "one 1", "other 2", "other 5", "other 21", "other 101"]],
  ] as const)("picks the %s form for 0, 1, 2, 5, 21 and 101", async (locale, expected) => {
    const before = language.tables;
    const table: Messages & FewForms = {
      ...en,
      "fileOps.files.one": "one {n}",
      "fileOps.files.few": "few {n}",
      "fileOps.files.other": "other {n}",
    };
    language.tables = { ...before, [locale]: table };
    try {
      await language.choose(locale, []);
      expect([0, 1, 2, 5, 21, 101].map((n) => tn("fileOps.files", n))).toEqual(expected);
    } finally {
      language.tables = before;
    }
  });

  // Polish and Ukrainian: CLDR "many" (5-21, 11-14, 0) reads `.other`; Polish counts 21 like 5, Ukrainian like 1
  it.each([
    ["pl", ["other 0", "one 1", "few 2", "other 5", "other 11", "other 12", "other 21", "few 22"]],
    ["uk", ["other 0", "one 1", "few 2", "other 5", "other 11", "other 12", "one 21", "few 22"]],
  ] as const)("picks the %s form for 0, 1, 2, 5, 11, 12, 21 and 22", async (locale, expected) => {
    const before = language.tables;
    const table: Messages & FewForms = {
      ...en,
      "fileOps.files.one": "one {n}",
      "fileOps.files.few": "few {n}",
      "fileOps.files.other": "other {n}",
    };
    language.tables = { ...before, [locale]: table };
    try {
      await language.choose(locale, []);
      expect([0, 1, 2, 5, 11, 12, 21, 22].map((n) => tn("fileOps.files", n))).toEqual(expected);
    } finally {
      language.tables = before;
    }
  });
});

describe("formatLocale", () => {
  it("follows the region the backend reports, ahead of the web view's languages, for the interface language only", async () => {
    try {
      language.region = "en-IN";
      await language.choose("en", ["en-US"]);
      expect(formatLocale()).toBe("en-IN");
      expect(sizeText(310e6)).toBe(
        new Intl.NumberFormat("en-IN", { style: "unit", unit: "megabyte" }).format(310),
      );
      await language.choose("de", ["en-US"]);
      expect(formatLocale()).toBe("de");
      language.region = null;
      await language.choose("system", ["en-GB"]);
      expect(formatLocale()).toBe("en-GB");
    } finally {
      language.region = null;
    }
  });
});

describe("matchLocale, systemLocale, resolveLocale", () => {
  it("reads a tag by its primary subtag, whatever the region", () => {
    expect(matchLocale("en-GB")).toBe("en");
    expect(matchLocale("pt-PT")).toBe("pt-PT");
    expect(matchLocale("es_AR")).toBe("es");
    expect(matchLocale("DE-at")).toBe("de");
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
    for (const tag of ["id", "id-ID", "ID_id"]) expect(matchLocale(tag), tag).toBe("id");
    for (const tag of ["vi", "vi-VN", "vi_VN"]) expect(matchLocale(tag), tag).toBe("vi");
    for (const tag of ["ru", "ru-RU", "ru_UA", "RU-kz"]) expect(matchLocale(tag), tag).toBe("ru");
    for (const tag of ["tr", "tr-TR", "tr_CY"]) expect(matchLocale(tag), tag).toBe("tr");
    for (const tag of ["pl", "pl-PL", "PL_pl"]) expect(matchLocale(tag), tag).toBe("pl");
    // Ukrainian never falls to Russian, wherever the region
    for (const tag of ["uk", "uk-UA", "uk_RU", "UK-ua"]) expect(matchLocale(tag), tag).toBe("uk");
  });

  it("reads Chinese by script, else by region: Traditional for Taiwan, Hong Kong and Macao", () => {
    for (const tag of [
      "zh-TW",
      "zh_TW",
      "zh-HK",
      "zh-MO",
      "ZH-hk",
      "zh-Hant",
      "zh-Hant-TW",
      "zh-Hant-CN",
    ]) {
      expect(matchLocale(tag), tag).toBe("zh-TW");
    }
    for (const tag of [
      "zh",
      "zh-CN",
      "zh_CN",
      "zh-SG",
      "zh-Hans",
      "zh-Hans-CN",
      "zh-Hans-TW",
      "zh-Hans-HK",
    ]) {
      expect(matchLocale(tag), tag).toBe("zh-CN");
    }
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
    expect(systemLocale(["uk-UA", "ru-RU"])).toBe("uk");
    expect(systemLocale([])).toBe("en");
  });

  it("writes dates and numbers as the system's region does when it speaks the interface language", () => {
    expect(formatTag("en", ["en-IN"])).toBe("en-IN");
    expect(formatTag("en", ["pt-BR", "en_AU", "en-GB"])).toBe("en-AU");
    expect(formatTag("zh-TW", ["zh-HK"])).toBe("zh-HK");
    expect(formatTag("pt-BR", ["pt-PT", "pt-BR"])).toBe("pt-BR");
    // another interface language, or a tag Intl cannot read, never counts
    expect(formatTag("pt-BR", ["pt-PT"])).toBe("pt-BR");
    expect(formatTag("de", ["en-GB", "fr-CH"])).toBe("de");
    expect(formatTag("en", ["en-IN-x-", "en-GB"])).toBe("en-GB");
    expect(formatTag("en", [])).toBe("en");
  });

  it("writes a language the runtime has no date | number data for as its stand-in, in the same region", () => {
    // WebView2 ships no Galician data: Intl would answer in English ("13 minutes ago", "1,167")
    const noGalician = (tag: string): boolean => matchLocale(tag) !== "gl";
    expect(formatTag("gl", ["gl-ES"], noGalician)).toBe("es-ES");
    expect(formatTag("gl", [], noGalician)).toBe("es");
    expect(formatTag("gl", ["gl-ES"], () => true)).toBe("gl-ES");
    // no stand-in, or none the runtime knows either: Intl's own default answers
    expect(formatTag("ca", ["ca-ES"], () => false)).toBe("ca-ES");
    expect(formatTag("gl", ["gl-ES"], () => false)).toBe("gl-ES");
  });

  it("asks the runtime itself which languages it can write dates, numbers and relative times in", async () => {
    const lacking = (tags?: Intl.LocalesArgument): string[] =>
      [tags ?? []]
        .flat()
        .map(String)
        .filter((tag) => matchLocale(tag) !== "gl");
    const spy = vi.spyOn(Intl.RelativeTimeFormat, "supportedLocalesOf").mockImplementation(lacking);
    try {
      expect(formatTag("gl", ["gl-ES"])).toBe("es-ES");
      expect(formatTag("ca", ["ca-ES"])).toBe("ca-ES");
      await language.choose("gl", ["gl-ES"]);
      expect(formatLocale()).toBe("es-ES");
    } finally {
      spy.mockRestore();
    }
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
    for (const tag of ["ind", "in", "vie", "rus", "tur", "tu", "zht", "zho-TW"]) {
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
      "zh-tw",
      "ZH-TW",
      "zh-Hant",
      "RU",
      "id-ID",
      "vi-VN",
      "tr-TR",
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

/**
 * `<base>.one` -> the placeholder(s) of the count that English writes out there ("1 file" beside "{n} files"): a table
 * may still write them in its `.one` form, which some languages also read for other counts.
 */
const COUNT = new Map(
  KEYS.filter((key) => key.endsWith(".one"))
    .map((key): [string, string[]] => {
      const own = placeholders(english[key] ?? "");
      const other = placeholders(english[`${key.slice(0, -".one".length)}.other`] ?? "");
      return [key, other.filter((p) => !own.includes(p))];
    })
    .filter(([, count]) => count.length > 0),
);

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
      const own = placeholders(english[key] ?? "");
      const count = COUNT.get(key) ?? [];
      const said = placeholders(value).filter((p) => own.includes(p) || !count.includes(p));
      expect(said, `${locale} ${key}`).toEqual(own);
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

  // Russian reads `.one` for 21 and 101, French and Brazilian Portuguese for 0: a literal "1" there says a wrong count
  it("says the count in a .one form the language also reads for other counts", async () => {
    const result = await loadTable(locale);
    if (result.status !== "loaded") throw new Error(`${locale}: ${result.status}`);
    const table: Record<string, string> = result.table;
    const rules = new Intl.PluralRules(locale);
    if (![0, 2, 5, 21, 101].some((n) => rules.select(n) === "one")) return;
    const wordless = [...COUNT].filter(
      ([key, count]) => !placeholders(table[key] ?? "").some((p) => count.includes(p)),
    );
    expect(wordless.map(([key]) => key)).toEqual([]);
  });
});

describe.each(LOCALES)("the %s table's typography", (locale) => {
  const table = async (): Promise<Record<string, string>> => {
    const result = await loadTable(locale);
    if (result.status !== "loaded") throw new Error(`${locale}: ${result.status}`);
    return result.table;
  };

  // shortcuts name the arrow keys with arrows in every language (`comboLabel`)
  it("names the back and forward keys with arrows", async () => {
    const words = await table();
    expect(
      [words["header.backHint"], words["follow.back"]].map((w) => w?.includes("Alt+←")),
    ).toEqual([true, true]);
    expect(words["header.forwardHint"]).toContain("Alt+→");
  });

  it("names the keys that move in a list with arrows", async () => {
    const words = await table();
    expect(words["finder.footMove"]).toMatch(/^↑\/↓ /);
  });

  // `[...]` and `{...}` are code (YAML flow style), not an ellipsis
  it("writes an ellipsis as one character", async () => {
    const words = await table();
    const dotted = Object.keys(words).filter((key) =>
      (words[key] ?? "").replace(/[[{]\.\.\.[\]}]/g, "").includes("..."),
    );
    expect(dotted).toEqual([]);
  });
});

describe("the words of a view moved to another region", () => {
  /** Per region: its name, the menu item that sends a view there, what is said once there, other lines naming it. */
  const REGIONS = [
    { name: "panel.sidebar", item: "panel.moveToLeft", said: "panel.moved.toLeft", more: [] },
    {
      name: "panel.right",
      item: "panel.moveToRight",
      said: "panel.moved.toRight",
      more: ["panel.tabs"],
    },
    {
      name: "panel.bottom",
      item: "panel.moveToBottom",
      said: "panel.moved.toBottom",
      more: ["panel.bottomTabs", "panel.bottomTooShort", "panel.moved.toBottomHidden"],
    },
  ] as const;
  const MOVED = KEYS.filter((key) => key.startsWith("panel.moved.") || key === "panel.closed");

  it("in English name the region, apart from the moves among the groups of one region", () => {
    expect([en["panel.moveToLeft"], en["panel.moveToRight"], en["panel.moveToBottom"]]).toEqual([
      "Move to sidebar",
      "Move to right panel",
      "Move to bottom panel",
    ]);
    expect([en["panel.moveLeft"], en["panel.moveRight"]]).toEqual(["Move left", "Move right"]);
    expect(
      [
        "panel.moved.toLeft",
        "panel.moved.toRight",
        "panel.moved.toBottom",
        "panel.moved.toBottomHidden",
        "panel.moved.onLeft",
        "panel.moved.onRight",
        "panel.closed",
      ].map((key) => english[key]),
    ).toEqual([
      "{name} moved to the sidebar",
      "{name} moved to the right panel",
      "{name} moved to the bottom panel",
      "{name} moved to the bottom panel. Make the window taller to show it.",
      "{name} now shows in its own group, on the left",
      "{name} now shows in its own group, on the right",
      "{name} closed",
    ]);
    expect(en["panel.bottomTooShort"]).toBe("Make the window taller to show the bottom panel.");
    expect(en["panel.undoLayoutCommand"]).toBe("Undo the last layout change");
  });

  it.each(LOCALES)("in %s use the word the region is called by", async (locale) => {
    const result = await loadTable(locale);
    if (result.status !== "loaded") throw new Error(`${locale}: ${result.status}`);
    const table: Record<string, string> = result.table;
    for (const { name, item, said, more } of REGIONS) {
      // the noun of the region's name (its last word; a name written without spaces is one word)
      const noun = (table[name] ?? "").split(/\s+/).at(-1)?.toLowerCase() ?? "";
      expect(noun, `${locale} ${name}`).not.toBe("");
      for (const key of [item, said, ...more]) {
        expect((table[key] ?? "").toLowerCase(), `${locale} ${key}`).toContain(noun);
      }
    }
  });

  // the Tree | Source group sits in the same bar as the View | Edit mode button: one name would read as one control
  it.each(LOCALES)(
    "in %s name the Tree | Source group apart from the View mode",
    async (locale) => {
      const result = await loadTable(locale);
      if (result.status !== "loaded") throw new Error(`${locale}: ${result.status}`);
      const table: Record<string, string> = result.table;
      expect(table["structured.mode"]?.toLowerCase()).not.toBe(table["mode.view"]?.toLowerCase());
    },
  );

  // where a participle | a verb would have to agree with the view's name (its gender, its number), the name stands
  // apart and nothing agrees with it
  it.each(["pt-BR", "pt-PT", "es", "fr", "it", "ca", "gl", "ro"] as const)(
    "in %s say a move without a word that agrees with the view's name",
    async (locale) => {
      const result = await loadTable(locale);
      if (result.status !== "loaded") throw new Error(`${locale}: ${result.status}`);
      const table: Record<string, string> = result.table;
      expect(MOVED.length).toBeGreaterThan(12);
      for (const key of MOVED) {
        expect(table[key], `${locale} ${key}`).toMatch(/^\{name\} ?: /);
      }
    },
  );
});

describe("the ja table's style", () => {
  it("writes ？ and ： full-width after Japanese text, and keeps the Git term worktree in English", async () => {
    const result = await loadTable("ja");
    if (result.status !== "loaded") throw new Error(`ja: ${result.status}`);
    const halfWidthAfterJapanese = /[\u3001-\u303f\u3040-\u30ff\u3400-\u9fff\uff01-\uffef][?:]/u;
    for (const [key, value] of Object.entries(result.table)) {
      expect(halfWidthAfterJapanese.test(value), `ja ${key}: ${value}`).toBe(false);
      expect(value, `ja ${key}`).not.toContain("\u30ef\u30fc\u30af\u30c4\u30ea\u30fc");
    }
  });
});

// one word per thing across a table: a reader who meets two words looks for two things
describe("the es table's words", () => {
  it("calls the settings Configuraci\u00f3n and speaks to the reader as t\u00fa, never usted", async () => {
    const result = await loadTable("es");
    if (result.status !== "loaded") throw new Error(`es: ${result.status}`);
    expect([result.table["header.settings"], result.table["settings.title"]]).toEqual([
      "Configuraci\u00f3n",
      "Configuraci\u00f3n",
    ]);
    // the usted forms the table once mixed in, the verbs as a clause's first word (mid-clause "guarde" can be a
    // third-person subjunctive); one setting ("ajuste") and a preset ("preajuste") stay
    const usted =
      /\busted\b|\ble (permite|pide)\b|(^|[.,;:]\s*)(elija|guarde|reinicie|descargue|edite)\b/iu;
    for (const [key, value] of Object.entries(result.table)) {
      expect(value, `es ${key}`).not.toMatch(/\bajustes\b/iu);
      expect(value, `es ${key}`).not.toMatch(usted);
    }
  });
});

describe("the plurals of the ru, pl and uk tables", () => {
  it.each(["ru", "pl", "uk"] as const)(
    "gives every counted %s string its few form (2-4, 22-24)",
    async (locale) => {
      const result = await loadTable(locale);
      if (result.status !== "loaded") throw new Error(`${locale}: ${result.status}`);
      const table: Record<string, string> = result.table;
      const missing = [...FEW.keys()].filter((key) => !(key in table));
      expect(missing).toEqual([]);
    },
  );
});

describe("the zh-CN table's words", () => {
  it("calls an AI agent \u667a\u80fd\u4f53 and a comment on a passage \u6279\u6ce8", async () => {
    const result = await loadTable("zh-CN");
    if (result.status !== "loaded") throw new Error(`zh-CN: ${result.status}`);
    for (const [key, value] of Object.entries(result.table)) {
      expect(value, `zh-CN ${key}`).not.toContain("\u4ee3\u7406");
      expect(value, `zh-CN ${key}`).not.toContain("\u8bc4\u8bba");
    }
  });
});

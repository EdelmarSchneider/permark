// Supported languages + how a preference | system tag resolves to one. Pure; reactive state in `language.svelte.ts`.
// Order = order in language pickers. File per locale in `locales/` (parity test checks list == directory).

export const LOCALES = [
  "en",
  "pt-BR",
  "es",
  "de",
  "zh-CN",
  "it",
  "ja",
  "ko",
  "fr",
  "pt-PT",
  "ca",
  "gl",
  "ro",
  "zh-TW",
  "id",
  "vi",
  "ru",
  "tr",
  "pl",
  "uk",
] as const;
export type Locale = (typeof LOCALES)[number];

/** Each language in its own name (pickers show autonyms; same in every interface language). */
export const LOCALE_NAMES: Record<Locale, string> = {
  en: "English",
  "pt-BR": "Português (Brasil)",
  es: "Español",
  de: "Deutsch",
  "zh-CN": "简体中文",
  it: "Italiano",
  ja: "日本語",
  ko: "한국어",
  fr: "Français",
  "pt-PT": "Português (Portugal)",
  ca: "Català",
  gl: "Galego",
  ro: "Română",
  "zh-TW": "繁體中文",
  id: "Bahasa Indonesia",
  vi: "Tiếng Việt",
  ru: "Русский",
  tr: "Türkçe",
  pl: "Polski",
  uk: "Українська",
};

/** Source language: complete table; answers whenever another table lacks a string. */
export const DEFAULT_LOCALE: Locale = "en";

/** Stored choice: a locale | follow the system. */
export type LanguagePreference = "system" | Locale;
export const DEFAULT_PREFERENCE: LanguagePreference = "system";

/** Chinese regions that write Traditional characters. */
const TRADITIONAL_REGIONS = new Set(["tw", "hk", "mo"]);

/**
 * BCP 47 | OS tag -> locale by primary subtag (`ja_JP` -> ja); else null. Portuguese also reads the region: `pt-BR` |
 * bare `pt` -> pt-BR, any other region (`pt-PT`, `pt-AO`, `pt-MZ`) -> pt-PT. Chinese reads the script, else the region:
 * `zh-Hant` | Taiwan, Hong Kong, Macao -> zh-TW; `zh-Hans` and any other Chinese -> zh-CN.
 */
export function matchLocale(tag: string): Locale | null {
  const [primary, ...subtags] = tag.trim().toLowerCase().split(/[-_]/);
  switch (primary) {
    case "en":
      return "en";
    case "pt": {
      const region = regionOf(subtags);
      return region === "" || region === "br" ? "pt-BR" : "pt-PT";
    }
    case "es":
      return "es";
    case "de":
      return "de";
    case "zh": {
      const script = scriptOf(subtags);
      if (script !== "") return script === "hant" ? "zh-TW" : "zh-CN";
      return TRADITIONAL_REGIONS.has(regionOf(subtags)) ? "zh-TW" : "zh-CN";
    }
    case "it":
      return "it";
    case "ja":
      return "ja";
    case "ko":
      return "ko";
    case "fr":
      return "fr";
    case "ca":
      return "ca";
    case "gl":
      return "gl";
    case "ro":
      return "ro";
    case "id":
      return "id";
    case "vi":
      return "vi";
    case "ru":
      return "ru";
    case "tr":
      return "tr";
    case "pl":
      return "pl";
    case "uk":
      return "uk";
    default:
      return null;
  }
}

/** Script subtag right after the language (`hant-tw` -> "hant"); "" when none. */
function scriptOf(subtags: readonly string[]): string {
  const [first = ""] = subtags;
  return /^[a-z]{4}$/.test(first) ? first : "";
}

/** Region subtag after the language, skipping a script (`latn-pt` -> "pt"); "" when none. */
function regionOf(subtags: readonly string[]): string {
  const [first = "", second = ""] = subtags;
  const region = scriptOf(subtags) === "" ? first : second;
  return /^(?:[a-z]{2}|\d{3})$/.test(region) ? region : "";
}

/** First system language the app speaks (person's order); English when none. */
export function systemLocale(tags: readonly string[]): Locale {
  for (const tag of tags) {
    const locale = matchLocale(tag);
    if (locale) return locale;
  }
  return DEFAULT_LOCALE;
}

/**
 * Language whose dates and numbers stand in for an interface language the runtime has no data for: one its readers
 * also read. WebView2 ships no Galician data (Intl would answer in English); every other interface language has it.
 */
const FORMAT_STAND_IN: Partial<Record<Locale, Locale>> = { gl: "es" };

/**
 * Whether the runtime writes dates, numbers and relative times of `tag` (asked where the code runs: a web view ships
 * data for fewer languages than Node's full ICU).
 */
function intlKnows(tag: string): boolean {
  try {
    return (
      Intl.NumberFormat.supportedLocalesOf([tag]).length > 0 &&
      Intl.DateTimeFormat.supportedLocalesOf([tag]).length > 0 &&
      Intl.RelativeTimeFormat.supportedLocalesOf([tag]).length > 0
    );
  } catch {
    return false;
  }
}

/**
 * Tag dates and numbers are written in: the first of `tags` (the system's format region, then its languages) that names
 * `locale` with a region of its own (`en-IN` while the interface speaks English), else `locale`. A tag of another
 * interface language never counts (`pt-PT` under pt-BR, `en-GB` under de). A tag the runtime has no data for (`known`)
 * gives way to its stand-in language in the same region (`gl-ES` -> `es-ES`) when the runtime knows that one.
 */
export function formatTag(
  locale: Locale,
  tags: readonly string[],
  known: (tag: string) => boolean = intlKnows,
): string {
  let found: string = locale;
  for (const tag of tags) {
    if (matchLocale(tag) !== locale) continue;
    const canonical = canonicalTag(tag);
    if (canonical === null) continue;
    found = canonical;
    break;
  }
  const standIn = FORMAT_STAND_IN[locale];
  if (standIn === undefined || known(found)) return found;
  const region = new Intl.Locale(found).region;
  const others = region === undefined ? [standIn] : [`${standIn}-${region}`, standIn];
  return others.find(known) ?? found;
}

/** `en_AU` | `en-au` -> `en-AU`; null for what Intl cannot read. */
function canonicalTag(tag: string): string | null {
  try {
    return Intl.getCanonicalLocales(tag.trim().replace(/_/g, "-"))[0] ?? null;
  } catch {
    return null;
  }
}

export function resolveLocale(preference: LanguagePreference, tags: readonly string[]): Locale {
  return preference === "system" ? systemLocale(tags) : preference;
}

/** Stored | received value -> preference; anything unknown -> "system". */
export function parsePreference(value: unknown): LanguagePreference {
  if (value === "system") return "system";
  return LOCALES.find((locale) => locale === value) ?? DEFAULT_PREFERENCE;
}

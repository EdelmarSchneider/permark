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
};

/** Source language: complete table; answers whenever another table lacks a string. */
export const DEFAULT_LOCALE: Locale = "en";

/** Stored choice: a locale | follow the system. */
export type LanguagePreference = "system" | Locale;
export const DEFAULT_PREFERENCE: LanguagePreference = "system";

/**
 * BCP 47 | OS tag -> locale by primary subtag (any Chinese -> zh-CN, `ja_JP` -> ja); else null. Portuguese also reads
 * the region: `pt-BR` | bare `pt` -> pt-BR, any other region (`pt-PT`, `pt-AO`, `pt-MZ`) -> pt-PT.
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
    case "zh":
      return "zh-CN";
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
    default:
      return null;
  }
}

/** Region subtag after the language, skipping a script (`latn-pt` -> "pt"); "" when none. */
function regionOf(subtags: readonly string[]): string {
  const [first = "", second = ""] = subtags;
  const region = /^[a-z]{4}$/.test(first) ? second : first;
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

export function resolveLocale(preference: LanguagePreference, tags: readonly string[]): Locale {
  return preference === "system" ? systemLocale(tags) : preference;
}

/** Stored | received value -> preference; anything unknown -> "system". */
export function parsePreference(value: unknown): LanguagePreference {
  if (value === "system") return "system";
  return LOCALES.find((locale) => locale === value) ?? DEFAULT_PREFERENCE;
}

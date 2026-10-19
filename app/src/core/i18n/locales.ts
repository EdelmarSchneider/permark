// Supported languages + how a preference | system tag resolves to one. Pure; reactive state in `language.svelte.ts`.
// Order = order in language pickers. File per locale in `locales/` (parity test checks list == directory).

export const LOCALES = ["en", "pt-BR", "es", "de", "zh-CN", "it"] as const;
export type Locale = (typeof LOCALES)[number];

/** Each language in its own name (pickers show autonyms; same in every interface language). */
export const LOCALE_NAMES: Record<Locale, string> = {
  en: "English",
  "pt-BR": "Português (Brasil)",
  es: "Español",
  de: "Deutsch",
  "zh-CN": "简体中文",
  it: "Italiano",
};

/** Source language: complete table; answers whenever another table lacks a string. */
export const DEFAULT_LOCALE: Locale = "en";

/** Stored choice: a locale | follow the system. */
export type LanguagePreference = "system" | Locale;
export const DEFAULT_PREFERENCE: LanguagePreference = "system";

/** BCP 47 tag | OS tag -> locale by primary subtag (`pt-PT` -> pt-BR; every Chinese tag -> zh-CN); else null. */
export function matchLocale(tag: string): Locale | null {
  const primary = tag.trim().toLowerCase().split(/[-_]/)[0];
  switch (primary) {
    case "en":
      return "en";
    case "pt":
      return "pt-BR";
    case "es":
      return "es";
    case "de":
      return "de";
    case "zh":
      return "zh-CN";
    case "it":
      return "it";
    default:
      return null;
  }
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

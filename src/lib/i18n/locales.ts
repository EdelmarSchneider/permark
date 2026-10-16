// The languages the interface comes in, and how a person's choice (or the system's language) turns into one of them.
// Pure functions only; the reactive state lives in `language.svelte.ts`.

export const LOCALES = ["en", "pt-BR", "es", "de", "zh-CN", "it"] as const;
export type Locale = (typeof LOCALES)[number];

/** The source language: its table is complete, and it answers whenever another table lacks a string. */
export const DEFAULT_LOCALE: Locale = "en";

/** What Settings stores: a language, or "follow the system". */
export type LanguagePreference = "system" | Locale;
export const LANGUAGE_PREFERENCES: readonly LanguagePreference[] = ["system", ...LOCALES];
export const DEFAULT_PREFERENCE: LanguagePreference = "system";

/** Each language written in itself, so a person can find theirs whatever the interface says now. */
export const LANGUAGE_NAMES: Record<Locale, string> = {
  en: "English",
  "pt-BR": "Português (Brasil)",
  es: "Español",
  de: "Deutsch",
  "zh-CN": "简体中文",
  it: "Italiano",
};

/** The BCP 47 tag `Intl` formats numbers and dates with. */
export const INTL_TAGS: Record<Locale, string> = {
  en: "en-US",
  "pt-BR": "pt-BR",
  es: "es",
  de: "de-DE",
  "zh-CN": "zh-CN",
  it: "it-IT",
};

/**
 * The language a browser/OS tag stands for by its primary part (`pt-PT` reads Portuguese too), or null. Every Chinese tag
 * (`zh-TW`, `zh-Hant`, `zh-HK` too) reads Simplified Chinese: it is the one Chinese the app has.
 */
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

/** The first of the system's languages (in the person's order) that the app speaks; English when none. */
export function systemLocale(languages: readonly string[]): Locale {
  for (const tag of languages) {
    const locale = matchLocale(tag);
    if (locale) return locale;
  }
  return DEFAULT_LOCALE;
}

export function resolveLocale(
  preference: LanguagePreference,
  systemLanguages: readonly string[],
): Locale {
  return preference === "system" ? systemLocale(systemLanguages) : preference;
}

/** A stored or received value as a preference; anything unknown reads as "system". */
export function parsePreference(value: unknown): LanguagePreference {
  return LANGUAGE_PREFERENCES.find((p) => p === value) ?? DEFAULT_PREFERENCE;
}

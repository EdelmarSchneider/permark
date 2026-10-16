import { de } from "./de";
import { en, type Messages } from "./en";
import { es } from "./es";
import { language } from "./language.svelte";
import { INTL_TAGS, type Locale } from "./locales";
import { ptBR } from "./ptBR";
import { zhCN } from "./zhCN";

export type MessageKey = keyof typeof en;
type PluralBase<K> = K extends `${infer B}.one` ? B : never;
/** Base name of a key that has `.one` and `.other` variants, e.g. "feed.docsChanged". */
export type PluralKey = PluralBase<MessageKey>;
export type Params = Record<string, string | number>;
export type { Messages };

export { LANGUAGE_NAMES, LANGUAGE_PREFERENCES, LOCALES, parsePreference } from "./locales";
export type { LanguagePreference, Locale } from "./locales";
export { language } from "./language.svelte";

/** Every language's table; English is the source and the fallback. */
export const MESSAGES: Record<Locale, Messages> = { en, "pt-BR": ptBR, es, de, "zh-CN": zhCN };

function fill(template: string, params?: Params): string {
  if (!params) return template;
  return template.replace(/\{(\w+)\}/g, (whole, name: string) =>
    name in params ? String(params[name]) : whole,
  );
}

function lookup(key: string): string {
  const table = MESSAGES[language.locale] as Partial<Record<string, string>>;
  return table[key] ?? (en as Record<string, string>)[key] ?? key;
}

/** The tag numbers, dates and plurals follow: the language of the interface. */
export function intlTag(): string {
  return INTL_TAGS[language.locale];
}

/** A number written the way the interface's language writes it ("1,200" or "1.200"). */
export function formatNumber(value: number): string {
  return value.toLocaleString(intlTag());
}

/** Items written as one list with the separator the interface's language uses: "a, b, c" or "a、b、c". */
export function joinList(items: readonly string[]): string {
  return items.join(t("list.separator"));
}

const pluralRules = new Map<string, Intl.PluralRules>();

/**
 * The plural category (`one`, `other`, ...) the interface's language gives a count. The rules are CLDR's, with one
 * exception: Brazilian usage writes zero in the plural ("0 arquivos"), where CLDR reads it as singular.
 */
export function pluralCategory(count: number): Intl.LDMLPluralRule {
  if (count === 0 && language.locale === "pt-BR") return "other";
  const tag = intlTag();
  let rules = pluralRules.get(tag);
  if (!rules) {
    rules = new Intl.PluralRules(tag);
    pluralRules.set(tag, rules);
  }
  return rules.select(count);
}

/** Looks up a UI string. Every user-facing string goes through here (English is the default). */
export function t(key: MessageKey, params?: Params): string {
  return fill(lookup(key), params);
}

/**
 * Looks up the form of a string that fits a count; `{n}` is filled with the count. A language needs a key for each
 * form it uses (`.one`, `.other`); a category it has no key for (`many` in Spanish and Portuguese, which only
 * applies to exact millions) reads as `.other`.
 */
export function tn(key: PluralKey, count: number, params?: Params): string {
  const category = pluralCategory(count);
  const table = MESSAGES[language.locale] as Partial<Record<string, string>>;
  const template = table[`${key}.${category}`] ?? lookup(`${key}.other`);
  return fill(template, { n: formatNumber(count), ...params });
}

/** What the keys are called on the keyboards of each language, where it differs from English (a key combo itself never changes). */
const KEY_NAMES: Partial<Record<Locale, Record<string, string>>> = {
  de: { Ctrl: "Strg", Shift: "Umschalt", Del: "Entf" },
  es: { Shift: "Mayús", Enter: "Intro", Del: "Supr" },
};

/** A key combo written for the interface's language: "Ctrl+Shift+O" is "Strg+Umschalt+O" in German. */
export function keys(combo: string): string {
  const names = KEY_NAMES[language.locale];
  if (!names) return combo;
  return combo.replace(/[A-Za-z]+/g, (name) => names[name] ?? name);
}

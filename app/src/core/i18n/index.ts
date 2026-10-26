// i18n entry: every user-facing string goes through `t()`.

import { language } from "./language.svelte";
import type { FewForms, MessageKey, Messages, Params } from "./messages";

export { language } from "./language.svelte";
export {
  LOCALE_NAMES,
  LOCALES,
  parsePreference,
  type LanguagePreference,
  type Locale,
} from "./locales";
export type { MessageKey, Messages, Params } from "./messages";

/** Fills `{name}` placeholders; unknown | missing names stay visible (never silently empty). */
export function fill(template: string, params?: Params): string {
  if (!params) return template;
  return template.replace(/\{(\w+)\}/g, (whole, name: string) => {
    // own keys only: "{constructor}" never reads Object.prototype
    return Object.hasOwn(params, name) ? String(params[name]) : whole;
  });
}

/** Keys that come in `.one` + `.other` pairs: `tn("fileOps.files", 3)`. */
export type PluralKey = {
  [K in MessageKey]: K extends `${infer Base}.one`
    ? `${Base}.other` extends MessageKey
      ? Base
      : never
    : never;
}[MessageKey];

/**
 * UI string for a count: the `.one` form when the language says "one", `.few` when it says "few" and the table has
 * one (Romanian), else `.other`; `{n}` is the count.
 */
export function tn(base: PluralKey, count: number, params?: Params): string {
  const category = new Intl.PluralRules(language.locale).select(count);
  const few =
    category === "few" ? (language.table as Messages & FewForms)[`${base}.few`] : undefined;
  const form = category === "one" ? "one" : "other";
  return fill(few ?? language.table[`${base}.${form}`], { n: count, ...params });
}

/** UI string in the active language (English fallback). */
export function t(key: MessageKey, params?: Params): string {
  return fill(language.table[key], params);
}

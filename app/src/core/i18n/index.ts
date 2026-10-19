// i18n entry: every user-facing string goes through `t()`.

import { language } from "./language.svelte";
import type { MessageKey, Params } from "./messages";

export { language } from "./language.svelte";
export { LOCALES, parsePreference, type LanguagePreference, type Locale } from "./locales";
export type { MessageKey, Messages, Params } from "./messages";

/** Fills `{name}` placeholders; unknown | missing names stay visible (never silently empty). */
export function fill(template: string, params?: Params): string {
  if (!params) return template;
  return template.replace(/\{(\w+)\}/g, (whole, name: string) => {
    // own keys only: "{constructor}" never reads Object.prototype
    return Object.hasOwn(params, name) ? String(params[name]) : whole;
  });
}

/** UI string in the active language (English fallback). */
export function t(key: MessageKey, params?: Params): string {
  return fill(language.table[key], params);
}

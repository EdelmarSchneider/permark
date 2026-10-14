import { en } from "./en";

export type MessageKey = keyof typeof en;
type PluralBase<K> = K extends `${infer B}.one` ? B : never;
/** Base name of a key that has `.one` and `.other` variants, e.g. "feed.docsChanged". */
export type PluralKey = PluralBase<MessageKey>;
export type Params = Record<string, string | number>;

/** Locale used for number and date formatting (the UI is English for now). */
export const LOCALE = "en-US";

function fill(template: string, params?: Params): string {
  if (!params) return template;
  return template.replace(/\{(\w+)\}/g, (whole, name: string) =>
    name in params ? String(params[name]) : whole,
  );
}

/** Looks up a UI string. Every user-facing string goes through here (English is the default). */
export function t(key: MessageKey, params?: Params): string {
  return fill(en[key], params);
}

/** Looks up the singular or plural form of a string; `{n}` is filled with the count. */
export function tn(key: PluralKey, count: number, params?: Params): string {
  const form = count === 1 ? "one" : "other";
  return fill(en[`${key}.${form}` as MessageKey], { n: count.toLocaleString(LOCALE), ...params });
}

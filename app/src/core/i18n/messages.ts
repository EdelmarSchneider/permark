import type en from "./locales/en";

/** Every UI string key; English table = source of truth. */
export type MessageKey = keyof typeof en;

/** Shape every locale table satisfies: exactly English's keys (compile-time parity). */
export type Messages = Record<MessageKey, string>;

/** `<base>.few` beside a `.one` key: the third plural form a table may add (Romanian "2 fișiere", "20 de fișiere"). */
export type FewForms = {
  [K in MessageKey as K extends `${infer Base}.one` ? `${Base}.few` : never]?: string;
};

/** Values for `{name}` placeholders. */
export type Params = Readonly<Record<string, string | number>>;

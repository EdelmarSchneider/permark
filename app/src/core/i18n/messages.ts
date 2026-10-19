import type en from "./locales/en";

/** Every UI string key; English table = source of truth. */
export type MessageKey = keyof typeof en;

/** Shape every locale table satisfies: exactly English's keys (compile-time parity). */
export type Messages = Record<MessageKey, string>;

/** Values for `{name}` placeholders. */
export type Params = Readonly<Record<string, string | number>>;

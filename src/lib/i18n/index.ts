import { en } from "./en";

export type MessageKey = keyof typeof en;

/** Looks up a UI string. Every user-facing string goes through here (English is the default). */
export function t(key: MessageKey): string {
  return en[key];
}

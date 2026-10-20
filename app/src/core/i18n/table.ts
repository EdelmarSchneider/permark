// Locale tables: English bundled; others discovered in `locales/` + loaded on demand (startup loads one table only).

import en from "./locales/en";
import type { Locale } from "./locales";
import type { Messages } from "./messages";

export { en };

type TableModule = { default: Messages };

const loaders = import.meta.glob<TableModule>(["./locales/*.ts", "!./locales/en.ts"]);

/** `./locales/pt-BR.ts` -> `pt-BR`. */
export function localeOfPath(path: string): string {
  return path.slice(path.lastIndexOf("/") + 1).replace(/\.ts$/, "");
}

/** Locale codes of the files in `locales/` (excluding en). */
export function discoveredLocales(): string[] {
  return Object.keys(loaders).map(localeOfPath);
}

export type LoadResult =
  | { status: "loaded"; table: Messages }
  | { status: "missing" }
  | { status: "failed"; error: unknown };

export async function loadTable(locale: Locale): Promise<LoadResult> {
  if (locale === "en") return { status: "loaded", table: en };
  const loader = loaders[`./locales/${locale}.ts`];
  if (!loader) return { status: "missing" };
  try {
    const module = await loader();
    return { status: "loaded", table: module.default };
  } catch (error) {
    return { status: "failed", error };
  }
}

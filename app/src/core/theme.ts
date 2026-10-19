// Look: theme (system | light | dark) + light tone, applied to `<html>` before first paint (no flash). Pure parts here;
// reactive state in `look.svelte.ts`.

import type { ThemePref, Tone } from "../api";

export type Theme = "light" | "dark";

export const DARK_QUERY = "(prefers-color-scheme: dark)";

export function themeFor(preference: ThemePref, systemDark: boolean): Theme {
  switch (preference) {
    case "system":
      return systemDark ? "dark" : "light";
    case "light":
      return "light";
    case "dark":
      return "dark";
    default:
      return preference satisfies never;
  }
}

/** `data-theme` + `data-tone` on the root (tokens.css keys off both; tone only affects light). */
export function applyLook(root: HTMLElement, theme: Theme, tone: Tone): void {
  root.dataset["theme"] = theme;
  root.dataset["tone"] = tone;
}

export function systemPrefersDark(): boolean {
  return typeof window !== "undefined" && window.matchMedia(DARK_QUERY).matches;
}

// Theme follows the system (setting comes with the settings slice). Applied before first paint (no flash).

export type Theme = "light" | "dark";

const DARK_QUERY = "(prefers-color-scheme: dark)";

export function themeFor(prefersDark: boolean): Theme {
  return prefersDark ? "dark" : "light";
}

export function applyTheme(root: HTMLElement, theme: Theme): void {
  root.dataset["theme"] = theme;
}

/** Current system theme, applied to `<html>`. */
export function applySystemTheme(): void {
  applyTheme(document.documentElement, themeFor(window.matchMedia(DARK_QUERY).matches));
}

/** Follows system theme changes. Returns teardown. */
export function watchSystemTheme(): () => void {
  const query = window.matchMedia(DARK_QUERY);
  const onChange = (event: MediaQueryListEvent): void => {
    applyTheme(document.documentElement, themeFor(event.matches));
  };
  query.addEventListener("change", onChange);
  return () => query.removeEventListener("change", onChange);
}

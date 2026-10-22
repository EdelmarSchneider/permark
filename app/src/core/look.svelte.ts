// Reactive look of this window: the person's theme + tone choice, the system's dark mode, applied on every change;
// whether long lines of text wrap (text viewer, source editors).

import type { AppSettings, ThemePref, Tone } from "../api";
import { applyLook, DARK_QUERY, systemPrefersDark, themeFor, type Theme } from "./theme";

class Look {
  preference = $state<ThemePref>("system");
  tone = $state<Tone>("white");
  systemDark = $state(systemPrefersDark());
  theme = $derived<Theme>(themeFor(this.preference, this.systemDark));
  /** Long lines wrap (setting `wrapLines`). */
  wrap = $state(true);

  /** From settings (boot, settings-changed); applies at once. */
  set(settings: Pick<AppSettings, "theme" | "tone" | "wrapLines">): void {
    this.preference = settings.theme;
    this.tone = settings.tone;
    this.wrap = settings.wrapLines;
    this.apply();
  }

  apply(): void {
    if (typeof document !== "undefined") applyLook(document.documentElement, this.theme, this.tone);
  }

  /** Follows the system's dark mode. Returns teardown. */
  watchSystem(): () => void {
    const query = window.matchMedia(DARK_QUERY);
    const onChange = (event: MediaQueryListEvent): void => {
      this.systemDark = event.matches;
      this.apply();
    };
    query.addEventListener("change", onChange);
    return () => query.removeEventListener("change", onChange);
  }
}

export const look = new Look();

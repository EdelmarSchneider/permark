// The language of the interface, as reactive state: every `t()` reads `language.locale`, so changing the preference
// redraws the text that is on screen without a restart.

import { DEFAULT_PREFERENCE, resolveLocale, type LanguagePreference, type Locale } from "./locales";

function systemLanguages(): readonly string[] {
  if (typeof navigator === "undefined") return [];
  return navigator.languages?.length ? navigator.languages : [navigator.language];
}

class LanguageState {
  /** The person's choice, or "system". */
  preference = $state<LanguagePreference>(DEFAULT_PREFERENCE);
  /** The languages the OS or the browser reports, in order of preference. */
  private systemTags = $state<readonly string[]>(systemLanguages());
  /** The language the interface is in right now. */
  locale = $derived<Locale>(resolveLocale(this.preference, this.systemTags));

  /** Follows the system's language while the preference is "system". */
  watchSystem(): void {
    window.addEventListener("languagechange", () => {
      this.systemTags = systemLanguages();
    });
  }
}

export const language = new LanguageState();

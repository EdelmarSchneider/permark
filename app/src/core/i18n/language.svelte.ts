// Interface language as reactive state: `t()` reads `language.locale` + loaded tables, so a change redraws text with no
// restart. A table is loaded BEFORE its locale becomes active (no English flash).

import { DEFAULT_PREFERENCE, resolveLocale, type LanguagePreference, type Locale } from "./locales";
import type { Messages } from "./messages";
import { en, loadTable } from "./table";

function navigatorTags(): readonly string[] {
  if (typeof navigator === "undefined") return [];
  return navigator.languages.length > 0 ? navigator.languages : [navigator.language];
}

class LanguageState {
  /** Person's choice | "system". */
  preference = $state<LanguagePreference>(DEFAULT_PREFERENCE);
  /** OS | browser languages, preferred first. */
  systemTags = $state.raw<readonly string[]>(navigatorTags());
  /** Loaded tables; missing locale -> English. */
  tables = $state.raw<Partial<Record<Locale, Messages>>>({ en });
  /** Active interface language. */
  locale = $derived<Locale>(resolveLocale(this.preference, this.systemTags));

  /** Table of the active locale; English until (or unless) it loads. */
  table = $derived<Messages>(this.tables[this.locale] ?? en);

  /** Loads the table of `locale` once; failure keeps English fallback. */
  async ensure(locale: Locale): Promise<void> {
    if (this.tables[locale]) return;
    const result = await loadTable(locale);
    switch (result.status) {
      case "loaded":
        this.tables = { ...this.tables, [locale]: result.table };
        return;
      case "missing":
      case "failed":
        return;
      default:
        result satisfies never;
    }
  }

  /** Sequence of `choose` calls: only the latest one may activate (rapid switches, out-of-order loads). */
  #latest = 0;

  /** Switches language: loads first, then activates; a newer call supersedes an older one still loading. */
  async choose(
    preference: LanguagePreference,
    tags: readonly string[] = this.systemTags,
  ): Promise<void> {
    const request = ++this.#latest;
    await this.ensure(resolveLocale(preference, tags));
    if (request !== this.#latest) return;
    this.preference = preference;
    this.systemTags = tags;
    // screen readers, hyphenation, CJK font selection
    if (typeof document !== "undefined") document.documentElement.lang = this.locale;
  }

  /** Follows OS language changes while preference = "system". Returns teardown. */
  watchSystem(): () => void {
    const onChange = (): void => {
      void this.choose(this.preference, navigatorTags());
    };
    window.addEventListener("languagechange", onChange);
    return () => window.removeEventListener("languagechange", onChange);
  }
}

export const language = new LanguageState();

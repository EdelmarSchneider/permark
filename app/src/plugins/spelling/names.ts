// Names of the spelling languages as the window shows them: the runtime's own names in the window's language (the
// translation plugin's naming, so a language reads the same in both), its region in parentheses. Lists take the full
// form ("Portuguese (Brazil)"); the status bar, where room is short, the region's code ("Portuguese (BR)").

import { language } from "../../core/i18n";
import { namesIn } from "../translation/languages";

/** `tag`'s name in a list | a sentence: "Portuguese (Brazil)", "English". */
export function languageLabel(tag: string, locale: string = language.locale): string {
  return namesIn(locale)(tag);
}

/** `tag`'s short name: its language, then its region's code ("Portuguese (BR)"). */
export function shortLabel(tag: string, locale: string = language.locale): string {
  const [base = tag, region] = tag.split("-");
  const name = namesIn(locale)(base);
  return region === undefined ? name : `${name} (${region})`;
}

/** The tags sorted by their names in `locale`. */
export function byName(tags: readonly string[], locale: string = language.locale): string[] {
  const name = namesIn(locale);
  const collator = new Intl.Collator(locale);
  return [...tags].sort((a, b) => collator.compare(name(a), name(b)));
}

// Languages a text can be translated into | from: a fixed list of BCP 47 tags, each named by the runtime's own data
// (`Intl.DisplayNames`): no name is typed by the person, none is translated here. The English name of a run's
// language goes into the model's prompt, so the backend checks tag and name before it takes them; `isTag`,
// `isPromptName` and `sameTag` are its rules (Rust `translation::prompt::{Tag, Language, Pair}`), kept here so a
// language it would refuse is never offered. A language's script (`scriptOf`) tells what an answer in it is written in.

import { language } from "../../core/i18n";
import { languageName } from "../local-ai/words";

/**
 * The languages offered (a list of them is sorted by name where it is shown). Plain language codes; a variant only
 * where the written standard differs (English, Portuguese, Spanish, Chinese, French, Serbian: there the bare code is
 * left out, except French). No catch-all code (`und`, `mul`, `zxx`), no sign language, no private-use tag. Which of
 * them a model handles, and how well, is the model's matter.
 */
export const LANGUAGES: readonly string[] = [
  "af",
  "sq",
  "am",
  "ar",
  "hy",
  "as",
  "az",
  "bn",
  "eu",
  "be",
  "bs",
  "br",
  "bg",
  "my",
  "yue",
  "ca",
  "ceb",
  "zh-Hans",
  "zh-Hant",
  "co",
  "hr",
  "cs",
  "da",
  "nl",
  "en-US",
  "en-GB",
  "eo",
  "et",
  "fo",
  "fil",
  "fi",
  "fr",
  "fr-CA",
  "gl",
  "ka",
  "de",
  "el",
  "gu",
  "ht",
  "ha",
  "haw",
  "he",
  "hi",
  "hmn",
  "hu",
  "is",
  "ig",
  "id",
  "ga",
  "it",
  "ja",
  "jv",
  "kn",
  "kk",
  "km",
  "rw",
  "ko",
  "ku",
  "ky",
  "lo",
  "la",
  "lv",
  "lt",
  "lb",
  "mk",
  "mg",
  "ms",
  "ml",
  "mt",
  "mi",
  "mr",
  "mn",
  "ne",
  "nb",
  "nn",
  "ny",
  "oc",
  "or",
  "ps",
  "fa",
  "pl",
  "pt-BR",
  "pt-PT",
  "pa",
  "qu",
  "ro",
  "rm",
  "ru",
  "sm",
  "gd",
  "sr-Cyrl",
  "sr-Latn",
  "sn",
  "sd",
  "si",
  "sk",
  "sl",
  "so",
  "st",
  "es-ES",
  "es-419",
  "su",
  "sw",
  "sv",
  "tg",
  "ta",
  "tt",
  "te",
  "th",
  "bo",
  "ti",
  "tr",
  "tk",
  "uk",
  "ur",
  "ug",
  "uz",
  "vi",
  "cy",
  "fy",
  "wo",
  "xh",
  "yi",
  "yo",
  "zu",
];

const TAG = /^[A-Za-z]{2,3}(-[A-Za-z0-9]{2,8})*$/;
const TAG_MAX = 35;
/** A language name: letters of any script and the few marks names are written with ("Serbian (Cyrillic)", "N'Ko").
 *  No period, no comma: no name the runtime gives has one, and with them a short sentence reads as a name. */
const NAME = /^[\p{Alphabetic} ()\-']+$/u;
const NAME_LETTER = /\p{Alphabetic}/u;
const NAME_MAX = 64;
/** Most words of a language name (the English names of the languages offered have 3 at most). */
const NAME_WORDS = 6;
/** The apostrophe the runtime's names are written with ("N’Ko"); the backend takes the plain one. */
const TYPOGRAPHIC_APOSTROPHE = /\u2019/g;

/** `tag` is shaped like a language tag as the backend takes one (shape alone: no registry behind it). */
export function isTag(tag: string): boolean {
  return tag.length <= TAG_MAX && TAG.test(tag);
}

/** `name` may stand in the model's prompt as a language's name: a letter at least, nothing that reads as more
 *  prompt (a line break, a quote, a colon, a digit, a period, a comma), 64 chars and 6 words (runs of anything but
 *  spaces) at most. */
export function isPromptName(name: string): boolean {
  return (
    NAME.test(name) &&
    NAME_LETTER.test(name) &&
    [...name].length <= NAME_MAX &&
    name.split(" ").filter((word) => word !== "").length <= NAME_WORDS
  );
}

/** Two tags name one language, whatever their case (`pt-br` = `pt-BR`). */
export function sameTag(a: string, b: string): boolean {
  return a.toLowerCase() === b.toLowerCase();
}

/** The runtime's language names in `locale` (`null`: it has none). */
function displayNames(locale: string): Intl.DisplayNames | null {
  try {
    return new Intl.DisplayNames(locale, { type: "language", languageDisplay: "dialect" });
  } catch {
    return null;
  }
}

/** Built on the first name asked for (never at app start). */
let english: Intl.DisplayNames | null | undefined;

/** The name the prompt shows for `tag`: its English dialect name ("Brazilian Portuguese"); `null` when the runtime
 *  has no name for it (it answers the code back) or the name is not one the backend takes. */
export function englishName(tag: string): string | null {
  if (english === undefined) english = displayNames("en");
  if (english === null) return null;
  const name = languageName(tag, english).replace(TYPOGRAPHIC_APOSTROPHE, "'");
  return sameTag(name, tag) || !isPromptName(name) ? null : name;
}

/** Built on the first list asked for. */
let nameable: readonly string[] | null = null;

/** The languages of `LANGUAGES` a run can ask for here: those the runtime names in English. */
export function offered(): readonly string[] {
  nameable ??= LANGUAGES.filter((tag) => englishName(tag) !== null);
  return nameable;
}

/** Where a language's name is shown: on its own (a list row, a tab, a menu entry, a label) | inside a sentence. */
export type NamePlace = "label" | "sentence";

/** Names of languages in the language `locale` (default: the app's): `tag` -> its name there, the tag itself where
 *  the runtime has none. The runtime writes a name as it stands inside a sentence ("alemão": small in Portuguese,
 *  Spanish, French, …); as a `label` its first letter is a capital, by that language's rules. One lookup serves a
 *  whole list. */
export function namesIn(
  locale: string = language.locale,
  place: NamePlace = "label",
): (tag: string) => string {
  const names = displayNames(locale);
  return (tag) => {
    const name = names === null ? tag : languageName(tag, names);
    if (place === "sentence" || sameTag(name, tag)) return name;
    // the first character whole (a letter outside the basic plane is two code units)
    const [first] = name;
    return first === undefined ? name : first.toLocaleUpperCase(locale) + name.slice(first.length);
  };
}

/** The name of `tag`'s language in that language itself, written as the runtime gives it ("Deutsch", "日本語",
 *  "português (Brasil)"); `null` where the runtime has no names in it (asked anyway, it answers in its own default
 *  language) or none for the tag. */
export function ownName(tag: string): string | null {
  try {
    if (Intl.DisplayNames.supportedLocalesOf([tag]).length === 0) return null;
  } catch {
    return null;
  }
  const name = namesIn(tag, "sentence")(tag);
  return sameTag(name, tag) ? null : name;
}

/** The script `tag`'s language is written in, as the runtime's locale data says (`Latn`, `Cyrl`, `Jpan`, `Hans`, …);
 *  `undefined` when it does not know. */
export function scriptOf(tag: string): string | undefined {
  try {
    return new Intl.Locale(tag).maximize().script;
  } catch {
    return undefined;
  }
}

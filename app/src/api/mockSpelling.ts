// What the spelling plugin answers in the mock backend: English ready with a small word list, the other languages
// absent until a download (a moment, can be made to fail), the personal words, the language of a document (declared,
// else guessed from common words, else the default). In memory; the same shapes as the real plugin.

import type { SpellingDefault } from "./generated/SpellingDefault";
import type { SpellingDocLanguage } from "./generated/SpellingDocLanguage";
import type { SpellingFailure } from "./generated/SpellingFailure";
import type { SpellingLanguage } from "./generated/SpellingLanguage";
import type { SpellingState } from "./generated/SpellingState";
import { MockPluginError } from "./mockOrganizing";
import type { AppPluginEventName, AppPluginEvents, PluginMethods } from "./types";

type SpellingMethods = Pick<
  PluginMethods,
  | "spelling.languages"
  | "spelling.download"
  | "spelling.cancel"
  | "spelling.check"
  | "spelling.suggest"
  | "spelling.personal.add"
  | "spelling.personal.remove"
  | "spelling.personal.list"
  | "spelling.language"
  | "spelling.declare"
  | "spelling.defaults.get"
  | "spelling.defaults.set"
>;

/** Same tags, licenses and sizes as the real table. */
const LANGUAGES: readonly { id: string; license: string; bytes: number }[] = [
  { id: "en", license: "(MIT AND BSD)", bytes: 554_848 },
  { id: "en-GB", license: "(MIT AND BSD)", bytes: 555_192 },
  { id: "pt-BR", license: "(LGPL-3.0 OR MPL-2.0)", bytes: 5_457_481 },
  { id: "pt-PT", license: "(GPL-2.0 OR LGPL-2.1 OR MPL-1.1)", bytes: 1_570_417 },
  { id: "es", license: "(GPL-3.0 OR LGPL-3.0 OR MPL-1.1)", bytes: 873_337 },
  { id: "fr", license: "MPL-2.0", bytes: 1_429_003 },
  { id: "de", license: "(GPL-2.0 OR GPL-3.0)", bytes: 1_137_393 },
  { id: "it", license: "GPL-3.0", bytes: 1_365_774 },
  { id: "ca", license: "(GPL-2.0 OR LGPL-2.1)", bytes: 2_926_530 },
  { id: "gl", license: "GPL-3.0", bytes: 9_796_316 },
  { id: "ro", license: "(GPL-2.0 OR LGPL-2.1 OR MPL-1.1)", bytes: 2_257_434 },
];

/** The stand-in dictionaries (lowercase; a word is also known Capitalized | in capitals). */
const WORDS: Record<string, readonly string[]> = {
  en: [
    "a",
    "an",
    "and",
    "the",
    "is",
    "it",
    "it's",
    "of",
    "to",
    "in",
    "on",
    "for",
    "with",
    "this",
    "that",
    "guide",
    "hello",
    "world",
    "document",
    "file",
    "files",
    "folder",
    "project",
    "read",
    "write",
    "text",
    "word",
    "words",
    "spelling",
    "check",
    "markdown",
    "notes",
    "note",
    "open",
    "save",
    "edit",
    "view",
  ],
  "pt-BR": [
    "a",
    "o",
    "e",
    "de",
    "um",
    "uma",
    "casa",
    "texto",
    "palavra",
    "documento",
    "olá",
    "mundo",
  ],
};

/** Common words that give a language away (the real plugin uses a statistical detector). */
const COMMON: Record<string, readonly string[]> = {
  en: ["the", "and", "is", "of", "to", "it", "on", "with", "very"],
  "pt-BR": ["o", "de", "um", "uma", "não", "que", "muito", "para", "foi"],
  es: ["el", "los", "las", "una", "y", "es", "muy", "pero", "del"],
  fr: ["le", "les", "et", "est", "une", "des", "très", "mais", "pas"],
  de: ["der", "die", "das", "und", "ist", "nicht", "ein", "eine", "sehr"],
  it: ["il", "che", "è", "della", "gli", "molto", "sono", "non", "una"],
};

/** Fewest words before a guess is believed. */
const MIN_WORDS = 8;

/** The script of a text no dictionary covers, by its first such letter. */
function unsupportedScript(text: string): string | null {
  if (/\p{Script=Hangul}/u.test(text)) return "hangul";
  if (/[\p{Script=Hiragana}\p{Script=Katakana}]/u.test(text)) return "kana";
  if (/\p{Script=Han}/u.test(text)) return "han";
  if (/\p{Script=Cyrillic}/u.test(text)) return "cyrillic";
  return null;
}

/** The language whose common words lead in `sample`, when it is long enough and one clearly leads. */
function guess(sample: string): string | null {
  const words = sample.toLowerCase().match(/\p{L}+/gu) ?? [];
  if (words.length < MIN_WORDS) return null;
  const scores = Object.entries(COMMON)
    .map(([lang, list]) => [lang, words.filter((w) => list.includes(w)).length] as const)
    .sort((a, b) => b[1] - a[1]);
  const [first, second] = scores;
  if (first === undefined || first[1] < 2 || (second !== undefined && second[1] * 2 > first[1]))
    return null;
  return first[0];
}

export type SpellingProbe = {
  /** Puts a dictionary in a state (as the backend would announce it). */
  setState(lang: string, state: SpellingState): void;
  /** The next download ends in this failure (`null`: it works). */
  failNext: SpellingFailure | null;
  /** How long a download takes, in steps of 150 ms. */
  steps: number;
  /** Calls made, `method arg`. */
  calls: string[];
  /** Declared languages by document path. */
  declared: Map<string, string>;
};

/** Lowercase `word` when it is the word itself, Capitalized or in capitals. */
function known(list: ReadonlySet<string>, word: string): boolean {
  if (list.has(word)) return true;
  const lower = word.toLowerCase();
  if (!list.has(lower)) return false;
  const capitalized = lower.charAt(0).toUpperCase() + lower.slice(1);
  return word === capitalized || word === lower.toUpperCase();
}

/** Whether `a` becomes `b` by one letter changed, added, removed or two neighbors swapped. */
function near(a: string, b: string): boolean {
  if (a === b || Math.abs(a.length - b.length) > 1) return false;
  if (a.length === b.length) {
    const diff = [...a].flatMap((c, i) => (c === b[i] ? [] : [i]));
    if (diff.length === 1) return true;
    const [i, j] = diff;
    return diff.length === 2 && i !== undefined && j === i + 1 && a[i] === b[j] && a[j] === b[i];
  }
  const [long, short] = a.length > b.length ? [a, b] : [b, a];
  for (let i = 0; i < long.length; i += 1) {
    if (long.slice(0, i) + long.slice(i + 1) === short) return true;
  }
  return false;
}

export function createMockSpelling(
  emit: <E extends AppPluginEventName>(event: E, payload: AppPluginEvents[E]) => void,
) {
  // The deep regression mode renders pages for golden screenshots: no dictionary is ready there, so the
  // mock's tiny word list never marks the corpus (spelling has tests of its own).
  const startReady = import.meta.env.MODE !== "deep";
  const states = new Map<string, SpellingState>(
    LANGUAGES.map((l) => [
      l.id,
      startReady && l.id === "en" ? { kind: "ready" } : { kind: "absent" },
    ]),
  );
  const personal = new Set<string>();
  const timers = new Map<string, ReturnType<typeof setInterval>>();
  const probe: SpellingProbe = {
    setState: (lang, state) => {
      stop(lang);
      set(lang, state);
    },
    failNext: null,
    steps: 4,
    calls: [],
    declared: new Map(),
  };
  let chosenDefault: string | null = null;

  function languages(): SpellingLanguage[] {
    return LANGUAGES.map((l) => ({ ...l, state: states.get(l.id) ?? { kind: "absent" } }));
  }

  function set(lang: string, state: SpellingState): void {
    states.set(lang, state);
    emit("spelling:download", { lang, state });
  }

  function stop(lang: string): void {
    const timer = timers.get(lang);
    if (timer !== undefined) clearInterval(timer);
    timers.delete(lang);
  }

  function languageOf(lang: string): (typeof LANGUAGES)[number] {
    const found = LANGUAGES.find((l) => l.id === lang);
    if (found === undefined) throw new MockPluginError("invalidData", lang, "lang");
    return found;
  }

  /** The word lists of the ready languages among `langs` (refusals as the real plugin). */
  function ready(langs: string[]): ReadonlySet<string>[] {
    for (const lang of langs) languageOf(lang);
    const lists = [...new Set(langs)]
      .filter((lang) => states.get(lang)?.kind === "ready")
      .map((lang) => new Set(WORDS[lang] ?? []));
    if (lists.length === 0) throw new MockPluginError("unavailable", null, "notReady");
    return lists;
  }

  /** The default language: the one picked, else the window's language (its first region), else English. */
  function defaultOf(uiLocale: string): SpellingDefault {
    if (chosenDefault !== null) return { lang: chosenDefault, chosen: true };
    const primary = uiLocale.split("-")[0]?.toLowerCase() ?? "";
    const found =
      LANGUAGES.find((l) => l.id.toLowerCase() === uiLocale.toLowerCase()) ??
      LANGUAGES.find((l) => primary !== "" && l.id.split("-")[0] === primary);
    return { lang: found?.id ?? "en", chosen: false };
  }

  function languageOfDoc(path: string, sample: string, uiLocale: string): SpellingDocLanguage {
    const declared = probe.declared.get(path);
    if (declared !== undefined) return { langs: [declared], source: "declared" };
    const script = unsupportedScript(sample);
    if (script !== null) return { langs: [], source: "unsupported", script };
    const lang = defaultOf(uiLocale).lang;
    const found = guess(sample);
    if (found === null) return { langs: [lang], source: "default" };
    // the region of Portuguese | English follows the default, as in the real plugin
    const regional =
      (found === "pt-BR" && lang === "pt-PT") || (found === "en" && lang === "en-GB")
        ? lang
        : found;
    return { langs: [regional], source: "detected", detected: regional };
  }

  function personalForm(word: string): string {
    const form = word.trim().normalize("NFC").replaceAll("’", "'");
    if (form === "" || /\s/u.test(form) || [...form].length > 100)
      throw new MockPluginError("invalidData", null, "word");
    return form;
  }

  function changed(): string[] {
    const list = [...personal].sort();
    emit("spelling:personal", list);
    return list;
  }

  const handlers: {
    [M in keyof SpellingMethods]: (args: PluginMethods[M]["args"]) => PluginMethods[M]["result"];
  } = {
    "spelling.languages": () => languages(),
    "spelling.download": ({ lang }) => {
      probe.calls.push(`download ${lang}`);
      const total = languageOf(lang).bytes;
      const kind = states.get(lang)?.kind;
      if (kind !== "absent" && kind !== "failed") return languages();
      set(lang, { kind: "downloading", done: 0, total });
      let step = 0;
      timers.set(
        lang,
        setInterval(() => {
          step += 1;
          if (step < probe.steps) {
            set(lang, {
              kind: "downloading",
              done: Math.floor((total * step) / probe.steps),
              total,
            });
            return;
          }
          stop(lang);
          const failure = probe.failNext;
          probe.failNext = null;
          set(lang, failure === null ? { kind: "ready" } : { kind: "failed", reason: failure });
        }, 150),
      );
      return languages();
    },
    "spelling.cancel": ({ lang }) => {
      probe.calls.push(`cancel ${lang}`);
      languageOf(lang);
      if (states.get(lang)?.kind === "downloading") {
        stop(lang);
        set(lang, { kind: "absent" });
      }
      return languages();
    },
    "spelling.check": ({ langs, words }) => {
      probe.calls.push(`check ${words.length}`);
      if (words.length > 5000) throw new MockPluginError("tooLarge", null, "words");
      const lists = ready(langs);
      return words.map((raw) => {
        const word = raw.normalize("NFC").replaceAll("’", "'");
        return known(personal, word) || lists.some((list) => known(list, word));
      });
    },
    "spelling.suggest": ({ langs, word }) => {
      probe.calls.push(`suggest ${word}`);
      const lower = word.toLowerCase();
      const out: string[] = [];
      for (const list of ready(langs)) {
        const found = [...list].filter((w) => near(lower, w)).slice(0, 5);
        for (const w of found) if (!out.includes(w)) out.push(w);
      }
      return out;
    },
    "spelling.personal.add": ({ word }) => {
      personal.add(personalForm(word));
      return changed();
    },
    "spelling.personal.remove": ({ word }) => {
      personal.delete(personalForm(word));
      return changed();
    },
    "spelling.personal.list": () => [...personal].sort(),
    "spelling.language": ({ path, sample, uiLocale }) => languageOfDoc(path, sample, uiLocale),
    "spelling.declare": ({ path, lang, sample, uiLocale }) => {
      probe.calls.push(`declare ${path} ${lang ?? "none"}`);
      if (lang === null) probe.declared.delete(path);
      else probe.declared.set(path, languageOf(lang).id);
      return languageOfDoc(path, sample ?? "", uiLocale ?? "en");
    },
    "spelling.defaults.get": ({ uiLocale }) => defaultOf(uiLocale),
    "spelling.defaults.set": ({ lang }) => {
      chosenDefault = languageOf(lang).id;
      return { lang: chosenDefault, chosen: true };
    },
  };

  return { handlers, probe };
}

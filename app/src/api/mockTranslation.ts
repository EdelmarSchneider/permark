// What the translation plugin answers in the mock backend: a mock piece run (`mockPieceRun.ts`) on the model of job
// "translation" (the run's languages checked as the backend checks them), the remembered targets (in memory) and the
// new file of a translated copy (into the mock project, never over a file; its name checked as the backend checks
// it).
//
// The mock model "translates" by writing each Latin letter another way, one for one, and leaving the rest as it is
// (markers, Markdown symbols, digits, spaces, line breaks). Target written in a script of `LETTERS` -> that script's
// letters. Any other target -> the Latin letters with a mark on every vowel, the mark picked by the target's tag: so a
// target whose script the mock cannot write (Thai) gets an answer in the wrong script, which the window discards. A
// text with nothing left to write another way comes back unchanged: translating a translation again into the same
// language shows the window's "already in that language".

import { nameOf, parentOf } from "../core/explorer/list";
import { isPromptName, isTag, sameTag, scriptOf } from "../plugins/translation/languages";
import type { TranslationLanguage } from "./generated/TranslationLanguage";
import type { TranslationPrefs } from "./generated/TranslationPrefs";
import { checkName, extensionOf } from "./mockNames";
import { MockPluginError } from "./mockOrganizing";
import { createMockPieceRun, type PieceRunProbe, type PieceRunWiring } from "./mockPieceRun";
import type { PluginMethods } from "./types";

type TranslationMethods = Pick<
  PluginMethods,
  | "translation.status"
  | "translation.run"
  | "translation.cancel"
  | "translation.prefs"
  | "translation.setPrefs"
  | "translation.save"
>;

/** What a translation run is sent beside its id + pieces. */
type RunLanguages = Omit<PluginMethods["translation.run"]["args"], "run" | "pieces">;

export type TranslationProbe = PieceRunProbe<RunLanguages> & {
  /** The remembered targets (`translation.prefs`); set to start with a target chosen. */
  prefs: TranslationPrefs;
};

/** The mock project's files, as far as a new file needs them. */
export type TranslationFiles = {
  /** Something is at `path` (in any case, as on Windows). */
  taken: (path: string) => boolean;
  add: (path: string, text: string) => void;
};

/** Recent targets kept (Rust `translation::prefs::RECENT_MAX`). */
const RECENT_MAX = 5;
/** Numbers tried for a taken file name. */
const NAME_TRIES = 999;

/** 26 letters of each script the mock model writes, for `a`…`z`. */
const LETTERS: Readonly<Record<string, string>> = {
  Cyrl: "абвгдежзийклмнопрстуфхцчшщ",
  Grek: "αβγδεζηθικλμνξοπρστυφχψωάέ",
  Arab: "ابتثجحخدذرزسشصضطظعغفقكلمنه",
  Hebr: "אבגדהוזחטיכלמנסעפצקרשתךםןף",
  Jpan: "あいうえおかきくけこさしすせそたちつてとなにぬねのは",
  Hans: "一二三四五六七八九十百千万日月火水木金土山川天地人口",
  Hant: "一二三四五六七八九十百千萬日月火水木金土山川天地人口",
  Kore: "가나다라마바사아자차카타파하고노도로모보소오조초코토",
};
/** Marks a vowel gets in a target written in Latin letters: acute, grave, circumflex, diaeresis. */
const MARKS = ["\u0301", "\u0300", "\u0302", "\u0308"];
/** A vowel with the mark it may already carry (decomposed text). */
const VOWEL = /([aeiou])[\u0300\u0301\u0302\u0308]?/gi;

/** The mock model's answer for `text` translated into the language of `tag` (rule: top of this file). */
export function mockTranslate(text: string, tag: string): string {
  const letters = [...(LETTERS[scriptOf(tag) ?? ""] ?? "")];
  if (letters.length > 0) {
    return text.replace(/[a-z]/gi, (ch) => letters[ch.toLowerCase().charCodeAt(0) - 97] ?? ch);
  }
  const sum = [...tag].reduce((total, ch, i) => total + ch.charCodeAt(0) * (i + 1), 0);
  const mark = MARKS[sum % MARKS.length] ?? "";
  return text.normalize("NFD").replace(VOWEL, `$1${mark}`).normalize("NFC");
}

// a language as the backend takes it (Rust `translation::prompt::Language::parse`)
function checked(language: TranslationLanguage): void {
  if (!isTag(language.tag)) throw new MockPluginError("invalidData", null, "tag");
  if (!isPromptName(language.name)) throw new MockPluginError("invalidData", null, "name");
}

// the name of a copy of `beside` as the backend takes it (Rust `translation::SaveArgs::checked`): one plain file
// name by the app's rule for names, with the extension of the original (an original without one: a copy without one)
function checkedCopy(beside: string, name: string): void {
  const refused = (detail: string) => new MockPluginError("invalidData", null, detail);
  if (checkName(name) !== null) throw refused("name");
  const original = nameOf(beside);
  if (original === "") throw refused("beside");
  if (extensionOf(name) !== extensionOf(original)) throw refused("extension");
}

// stem + extension of a file name, cut at its last dot (Rust `edit::split_name`)
function splitName(name: string): [string, string] {
  const dot = name.lastIndexOf(".");
  return dot > 0 ? [name.slice(0, dot), name.slice(dot)] : [name, ""];
}

export function createMockTranslation(
  wiring: PieceRunWiring,
  files: TranslationFiles,
): {
  handlers: {
    [M in keyof TranslationMethods]: (
      args: TranslationMethods[M]["args"],
    ) => TranslationMethods[M]["result"];
  };
  probe: TranslationProbe;
} {
  const runs = createMockPieceRun<RunLanguages>(
    "translation",
    "translation",
    (text, sent) => mockTranslate(text, sent.target.tag),
    wiring,
  );
  const probe: TranslationProbe = Object.assign(runs.probe, {
    prefs: { target: null, recent: [] },
  });
  return {
    probe,
    handlers: {
      "translation.status": runs.status,
      "translation.run": (sent) => {
        checked(sent.target);
        if (sent.source !== null) {
          checked(sent.source);
          if (sameTag(sent.source.tag, sent.target.tag))
            throw new MockPluginError("invalidData", null, "sameLanguage");
        }
        return runs.run(sent);
      },
      "translation.cancel": runs.cancel,
      "translation.prefs": () => structuredClone(probe.prefs),
      "translation.setPrefs": ({ target }) => {
        if (!isTag(target)) throw new MockPluginError("invalidData", null, "tag");
        const others = probe.prefs.recent.filter((known) => !sameTag(known, target));
        probe.prefs = { target, recent: [target, ...others].slice(0, RECENT_MAX) };
        return structuredClone(probe.prefs);
      },
      "translation.save": ({ beside, name, text }) => {
        checkedCopy(beside, name);
        const folder = parentOf(beside);
        const [stem, ext] = splitName(name);
        for (let n = 1; n <= NAME_TRIES; n += 1) {
          const candidate = n === 1 ? name : `${stem} (${n})${ext}`;
          const path = folder === "" ? candidate : `${folder}/${candidate}`;
          if (files.taken(path)) continue;
          files.add(path, text);
          return { path };
        }
        throw new MockPluginError("exists", name);
      },
    },
  };
}

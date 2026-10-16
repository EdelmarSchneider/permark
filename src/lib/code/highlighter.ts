// Syntax highlighting for fenced code, with every language highlight.js ships and none of them in the main bundle: the core and each
// grammar are separate chunks, fetched the first time a block asks for that language (a Vite glob of the grammar files). The
// reading view and Visual mode both draw with this.
//
// A block that names a language gets that language. A block that names none (or one that highlight.js does not have) is looked at:
// when the text itself gives the language away (a shebang, JSON that parses, an XML prolog, a diff) or highlight.js's own guess is
// both strong and clearly ahead of the runner-up, it is highlighted and labelled as a guess; otherwise it stays plain. A wrong
// colouring is worse than none, so the bar is high.

import type { HLJSApi, LanguageFn } from "highlight.js";
import { fenceLanguage, fenceWord, grammarName, isPlainName, resolveLanguage } from "./languages";

type Loader = () => Promise<LanguageFn>;

// one lazy import per grammar file; the grammar's id is its file name
// (each grammar has a `.js.js` twin that only warns about the extension: not those)
const GLOB = import.meta.glob<LanguageFn>(
  [
    "../../../node_modules/highlight.js/es/languages/*.js",
    "!../../../node_modules/highlight.js/es/languages/*.js.js",
  ],
  { import: "default" },
);

/** The lazy loader of every grammar, by id. */
export const GRAMMARS: ReadonlyMap<string, Loader> = new Map(
  Object.entries(GLOB).map(([path, load]) => [path.replace(/^.*\/([^/]+)\.js$/, "$1"), load]),
);

// ---- loading ------------------------------------------------------------------------------------

let core: Promise<HLJSApi> | null = null;
/** The core once it has loaded: Visual mode highlights while the person types, and cannot wait for a promise on every key. */
let loadedCore: HLJSApi | null = null;

function loadCore(): Promise<HLJSApi> {
  core ??= import("highlight.js/lib/core").then((module) => {
    loadedCore = module.default;
    return module.default;
  });
  return core;
}

const grammars = new Map<string, Promise<boolean>>();

/** Loads the grammar of `id` (and the core). Resolves to whether highlight.js can highlight that language now. */
export function ensureLanguage(id: string): Promise<boolean> {
  let loading = grammars.get(id);
  if (!loading) {
    const load = GRAMMARS.get(id);
    loading = load
      ? Promise.all([loadCore(), load()])
          .then(([hljs, grammar]) => {
            if (!hljs.getLanguage(id)) hljs.registerLanguage(id, grammar);
            return true;
          })
          .catch(() => {
            // a chunk that would not load can be asked for again later
            grammars.delete(id);
            return false;
          })
      : Promise.resolve(false);
    grammars.set(id, loading);
  }
  return loading;
}

/** Whether the grammar of `id` is loaded, so that `highlightLoaded` can answer at once. */
export function isLoaded(id: string): boolean {
  return loadedCore?.getLanguage(id) !== undefined;
}

/** Longest text that is highlighted: past this the block stays plain (the page must not stall on a pasted log). */
export const MAX_HIGHLIGHT_CHARS = 200_000;

/** highlight.js's HTML for `source` in a grammar that is already loaded, or null when it is not (or the text is too long). */
export function highlightLoaded(source: string, id: string): string | null {
  if (!loadedCore || !loadedCore.getLanguage(id) || source.length > MAX_HIGHLIGHT_CHARS) {
    return null;
  }
  try {
    return loadedCore.highlight(source, { language: id, ignoreIllegals: true }).value;
  } catch {
    return null;
  }
}

// ---- guessing -----------------------------------------------------------------------------------

/** The languages a block with no name is compared with: the ones that turn up most in docs, and that tell each other apart. */
export const AUTO_SUBSET: readonly string[] = [
  "bash",
  "c",
  "cpp",
  "csharp",
  "css",
  "diff",
  "dockerfile",
  "go",
  "ini",
  "java",
  "javascript",
  "json",
  "kotlin",
  "lua",
  "php",
  "powershell",
  "python",
  "ruby",
  "rust",
  "sql",
  "swift",
  "typescript",
  "xml",
  "yaml",
];

/** Shorter than this a block says too little to tell (a command, a path). */
export const AUTO_MIN_CHARS = 24;
/** Longer than this it is not guessed at (every candidate language reads the whole text). */
export const AUTO_MAX_CHARS = 8000;
/** highlight.js's relevance for the best language must reach this... */
export const AUTO_MIN_RELEVANCE = 10;
/** ...and be at least this many times the runner-up's. */
export const AUTO_MIN_RATIO = 1.5;

/** Whether highlight.js's best guess is strong enough, and far enough ahead of the next one, to be shown. */
export function acceptGuess(best: number, second: number): boolean {
  return best >= AUTO_MIN_RELEVANCE && (second <= 0 || best >= second * AUTO_MIN_RATIO);
}

const SHEBANG_LANGUAGES: Readonly<Record<string, string>> = {
  bash: "bash",
  sh: "bash",
  zsh: "bash",
  dash: "bash",
  ksh: "bash",
  python: "python",
  python3: "python",
  pypy: "python",
  node: "javascript",
  deno: "javascript",
  bun: "javascript",
  ruby: "ruby",
  perl: "perl",
  php: "php",
  pwsh: "powershell",
  powershell: "powershell",
};

/**
 * The language a text gives away by its shape, with no guessing: a `#!` line, JSON that parses, an XML or HTML prolog, a unified diff.
 * Null when it does not.
 */
export function sniffLanguage(source: string): string | null {
  const text = source.trim();
  const shebang = /^#!\s*(\S+)(?:[ \t]+(?:-\S+[ \t]+)*(\S+))?/.exec(text);
  if (shebang) {
    const basename = (path: string): string => path.split("/").pop() ?? "";
    // `#!/usr/bin/env python3` names the program after `env`; `python3.11` is Python
    const program = basename(basename(shebang[1]) === "env" ? (shebang[2] ?? "") : shebang[1]);
    const id = SHEBANG_LANGUAGES[program.replace(/[\d.]+$/, "")];
    if (id) return id;
  }
  if (/^[[{]/.test(text) && /[\]}]$/.test(text) && text.length <= MAX_HIGHLIGHT_CHARS) {
    try {
      JSON.parse(text);
      return "json";
    } catch {
      // not JSON
    }
  }
  if (/^(?:<\?xml\b|<!doctype\s+html|<html[\s>])/i.test(text)) return "xml";
  if (/^(?:diff --git |--- \S.*\n\+\+\+ \S|@@ -\d+(?:,\d+)? \+\d+(?:,\d+)? @@)/.test(text)) {
    return "diff";
  }
  if (looksLikePython(text)) return "python";
  return null;
}

/**
 * What only Python writes. highlight.js cannot tell short Python from Ruby, Go or CSS, and it is the language docs show most, so a text
 * is Python when it has both a line that opens a `def`, a `class`, an import or the `__main__` check, and a line that only Python
 * has: a block opened by a colon (`if x:`, `else:`), `self.` or `None`/`True`/`False`. Strict on purpose: a script that has only
 * one of the two stays plain, and so does Ruby, Swift, Kotlin or a YAML file.
 */
const PYTHON_OPENINGS: readonly RegExp[] = [
  /^[ \t]*(?:async )?def \w+\s*\([^)]*\)\s*(?:->[^:\n]+)?:[ \t]*$/m,
  /^[ \t]*class \w+\s*(?:\([^)]*\))?:[ \t]*$/m,
  /^(?:from [\w.]+ import [\w*, ]+|import [\w.]+(?: as \w+)?(?:, [\w.]+(?: as \w+)?)*)[ \t]*$/m,
  /^if __name__ == ["']__main__["']:/m,
];
const PYTHON_TRAITS: readonly RegExp[] = [
  /^[ \t]*(?:if|elif|for|while|with) [^\n]*:[ \t]*$/m,
  /^[ \t]*(?:else|try|finally|except(?: [^\n]*)?):[ \t]*$/m,
  /\bself\.\w+/,
  /\b(?:None|True|False)\b/,
];

/** Whether a text is Python by the patterns above. */
export function looksLikePython(text: string): boolean {
  return (
    PYTHON_OPENINGS.some((pattern) => pattern.test(text)) &&
    PYTHON_TRAITS.some((pattern) => pattern.test(text))
  );
}

/** The language of a block that names none, when it can be told with confidence. */
async function guessLanguage(source: string): Promise<string | null> {
  if (source.trim().length < AUTO_MIN_CHARS || source.length > AUTO_MAX_CHARS) return null;
  const sniffed = sniffLanguage(source);
  if (sniffed) return (await ensureLanguage(sniffed)) ? sniffed : null;
  const loaded = await Promise.all(AUTO_SUBSET.map(ensureLanguage));
  const subset = AUTO_SUBSET.filter((_, i) => loaded[i]);
  const hljs = await loadCore();
  if (subset.length === 0) return null;
  try {
    const result = hljs.highlightAuto(source, subset);
    if (!result.language) return null;
    return acceptGuess(result.relevance, result.secondBest?.relevance ?? 0)
      ? result.language
      : null;
  } catch {
    return null;
  }
}

// ---- a block ------------------------------------------------------------------------------------

/** What a code block shows: highlight.js's HTML for its text (null: plain), and the label of its corner (null: none). */
export type CodeOutcome = {
  html: string | null;
  label: { name: string; auto: boolean } | null;
};

const PLAIN: CodeOutcome = { html: null, label: null };

/**
 * Decides how a fenced block is drawn. `info` is what follows the backticks (the first word is the language).
 * - A language highlight.js has: highlighted, labelled with its name.
 * - A plain-text name (`text`, `txt`): plain, no label.
 * - A name highlight.js does not have, or none: guessed when that can be done with confidence ("auto: Python"); otherwise plain, and
 *   labelled with the name as the author wrote it when there is one.
 */
export async function highlightFence(source: string, info: string): Promise<CodeOutcome> {
  const word = fenceWord(info);
  const name = fenceLanguage(info);
  if (word !== "" && isPlainName(name)) return PLAIN;
  const resolved = resolveLanguage(name);
  if (resolved) {
    const ready = source.length <= MAX_HIGHLIGHT_CHARS && (await ensureLanguage(resolved.id));
    return {
      html: ready ? highlightLoaded(source, resolved.id) : null,
      label: { name: resolved.name, auto: false },
    };
  }
  const guess = await guessLanguage(source);
  if (guess) {
    const html = highlightLoaded(source, guess);
    if (html !== null) return { html, label: { name: grammarName(guess), auto: true } };
  }
  return word === "" ? PLAIN : { html: null, label: { name: word, auto: false } };
}

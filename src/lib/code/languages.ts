// The languages a fenced code block can name: reading the info string after the backticks, resolving what an author writes
// ("py", "c#", "ps1", "{python}") to a highlight.js grammar, the name shown for it, and the search behind the language picker.
// Pure data and string work: no grammar is loaded here (see highlighter.ts), so this module is cheap to import anywhere.

import { HLJS_LANGUAGES } from "./languageData";

/** Names an author writes for "no language": the block is plain text, never highlighted and never guessed. */
const PLAIN_NAMES: ReadonlySet<string> = new Set([
  "text",
  "txt",
  "plain",
  "plaintext",
  "none",
  "nohighlight",
  "raw",
]);

/**
 * Names that mean a grammar highlight.js has under another name, or no name for. They win over highlight.js's own aliases:
 * a fence named `shell` holds a script, where highlight.js's `shell` is a console session (that one is `console`).
 */
const EXTRA_ALIASES: Readonly<Record<string, string>> = {
  shell: "bash",
  shellscript: "bash",
  ksh: "bash",
  fish: "bash",
  dash: "bash",
  bashrc: "bash",
  zshrc: "bash",
  svelte: "xml",
  vue: "xml",
  astro: "xml",
  htm: "xml",
  xaml: "xml",
  csproj: "xml",
  mdx: "markdown",
  node: "javascript",
  nodejs: "javascript",
  jsonl: "json",
  ndjson: "json",
  geojson: "json",
  sass: "scss",
  mysql: "sql",
  mariadb: "sql",
  sqlite: "sql",
  tsql: "sql",
  psql: "pgsql",
  gemfile: "ruby",
  rakefile: "ruby",
  rake: "ruby",
  cuda: "cpp",
  cu: "cpp",
  rkt: "scheme",
  racket: "scheme",
  elisp: "lisp",
  "emacs-lisp": "lisp",
  commonlisp: "lisp",
  cl: "lisp",
};

/** Display names for the aliases whose grammar has a longer or different name ("HTML, XML", "TOML, also INI"). */
const ALIAS_NAMES: Readonly<Record<string, string>> = {
  html: "HTML",
  xhtml: "XHTML",
  xml: "XML",
  svg: "SVG",
  svelte: "Svelte",
  vue: "Vue",
  astro: "Astro",
  toml: "TOML",
  ini: "INI",
  jsonc: "JSONC",
  json5: "JSON5",
  sh: "Shell",
  shell: "Shell",
  zsh: "Zsh",
  fish: "Fish",
  yml: "YAML",
  mdx: "MDX",
  jsx: "JSX",
  tsx: "TSX",
  csproj: "XML",
  xaml: "XAML",
  mysql: "MySQL",
  sqlite: "SQLite",
  postgres: "PostgreSQL",
  postgresql: "PostgreSQL",
  psql: "PostgreSQL",
  ps1: "PowerShell",
  pwsh: "PowerShell",
  gql: "GraphQL",
  "objective-c": "Objective-C",
  objc: "Objective-C",
};

/** Names for grammars whose own name reads badly. */
const ID_NAMES: Readonly<Record<string, string>> = {
  php: "PHP",
  "python-repl": "Python REPL",
  xml: "HTML, XML",
  ini: "INI, TOML",
};

/** A language the editor and the reading view can highlight. */
export type CodeLanguage = {
  /** The name of its highlight.js grammar. */
  id: string;
  /** Its name for a person ("TypeScript"). */
  name: string;
  /** Every other name it answers to, lower case. */
  aliases: readonly string[];
};

/** What a typed name stands for. */
export type Resolved = { id: string; name: string };

const byId = new Map<string, CodeLanguage>();
for (const [id, name, aliases] of HLJS_LANGUAGES) {
  byId.set(id, { id, name: ID_NAMES[id] ?? name, aliases: [...aliases] });
}

// every name a fence may carry, lower case, to the grammar it means: ids first, then highlight.js's aliases, then ours
const names = new Map<string, string>();
for (const { id } of byId.values()) names.set(id, id);
for (const { id, aliases } of byId.values()) {
  for (const alias of aliases) {
    const key = alias.toLowerCase();
    if (!names.has(key)) names.set(key, id);
  }
}
for (const [alias, id] of Object.entries(EXTRA_ALIASES)) {
  if (byId.has(id)) names.set(alias, id);
}
// our own aliases are listed on the language they mean, so the picker finds it by them
for (const [alias, id] of Object.entries(EXTRA_ALIASES)) {
  const language = byId.get(id);
  if (language && alias !== id && !language.aliases.includes(alias)) {
    byId.set(id, { ...language, aliases: [...language.aliases, alias] });
  }
}
// a language is not its own alias, and the console session is reached as `console`
for (const language of byId.values()) {
  byId.set(language.id, {
    ...language,
    aliases: language.aliases.filter((alias) => alias !== language.id),
  });
}

/** Whether `name` (as written in a fence) means "no language". */
export function isPlainName(name: string): boolean {
  return PLAIN_NAMES.has(name.toLowerCase());
}

/**
 * The language of a fence's info string: its first word, lower case. Whatever follows (`title="x"`, `{1,3}`, `ignore`) is
 * the author's own business. `{python}`, `{.python .numberLines}`, `rust,ignore` and `js:app.js` all name one language.
 */
export function fenceLanguage(info: string): string {
  return fenceWord(info).toLowerCase();
}

/** The first word of an info string as it was written (see `fenceLanguage`). */
export function fenceWord(info: string): string {
  const word = info
    .trim()
    .replace(/^\{\s*\.?/, "")
    .replace(/^language-/i, "")
    .split(/[\s,{}:;]/, 1)[0];
  return word.replace(/^\./, "");
}

/** The grammar a name stands for, or null when highlight.js has none (and the name is not a plain-text name either). */
export function resolveLanguage(name: string): Resolved | null {
  const key = fenceLanguage(name);
  const id = names.get(key);
  if (id === undefined) return null;
  const language = byId.get(id);
  if (!language) return null;
  return { id, name: ALIAS_NAMES[key] ?? language.name };
}

/** The name of a grammar (by id) as a person reads it, for a language nobody named: "HTML, XML", "INI, TOML". */
export function grammarName(id: string): string {
  return byId.get(id)?.name ?? id;
}

/** The language of a name for display: its proper name when it is known, else the name as it was written. */
export function languageDisplayName(name: string): string {
  return resolveLanguage(name)?.name ?? fenceWord(name);
}

/** Every grammar, by name. */
export function allLanguages(): CodeLanguage[] {
  return [...byId.values()].filter((language) => language.id !== "plaintext");
}

/** The ids of the grammars highlight.js ships (the lazy loader must have one file for each). */
export function grammarIds(): string[] {
  return [...byId.keys()];
}

// ---- the picker ---------------------------------------------------------------------------------

/** What goes into the info string for a language picked from the list: its id, or the first alias that is not taken by another language. */
export function fenceNameOf(language: CodeLanguage): string {
  if (names.get(language.id) === language.id) return language.id;
  return language.aliases.find((alias) => names.get(alias) === language.id) ?? language.id;
}

/** Info-string text as typed in the picker: what an info string cannot hold (line breaks, backticks) is left out. */
export function sanitizeInfo(text: string): string {
  return text.replace(/[\r\n`]+/g, " ").trim();
}

export type PickerRow =
  | { kind: "plain" }
  | { kind: "language"; language: CodeLanguage; write: string; via: string | null }
  | { kind: "custom"; write: string };

/** Words that also find the "Plain text" row. */
const PLAIN_WORDS = ["plain text", "text", "none", "plain"];

/**
 * Rank of a language for a query: lower is better (0: the query is a name the language answers to in a fence), null when it does not
 * match. `via` is the alias that matched, when it was one.
 */
function rank(language: CodeLanguage, q: string): { score: number; via: string | null } | null {
  const name = language.name.toLowerCase();
  const found: { score: number; via: string | null }[] = [];
  // a name that a fence reads as another language (`shell` is Bash) is not an exact match for this one
  if (language.id === q && names.get(q) === language.id) found.push({ score: 0, via: null });
  else if (language.id.startsWith(q)) found.push({ score: 2, via: null });
  else if (language.id.includes(q)) found.push({ score: 4, via: null });
  for (const alias of language.aliases) {
    if (alias === q && names.get(q) === language.id) found.push({ score: 0, via: alias });
    else if (alias.startsWith(q)) found.push({ score: 2, via: alias });
    else if (alias.includes(q)) found.push({ score: 4, via: alias });
  }
  if (name === q) found.push({ score: 1, via: null });
  else if (name.split(/[\s,/:.-]+/).some((word) => word.startsWith(q)))
    found.push({ score: 3, via: null });
  else if (name.includes(q)) found.push({ score: 4, via: null });
  found.sort((a, b) => a.score - b.score);
  return found[0] ?? null;
}

/**
 * The rows of the language picker for what was typed. A language that the text names exactly (its id or an alias) writes
 * the text as it was typed (`js` stays `js`); any other match writes the language's own name. Text that no language is
 * named by gets a row of its own that writes exactly that text: the info string is free text.
 */
export function pickerRows(query: string, limit = 60): PickerRow[] {
  const typed = sanitizeInfo(query);
  const q = typed.toLowerCase();
  const rows: PickerRow[] = [];
  if (q === "") {
    rows.push({ kind: "plain" });
    for (const language of allLanguages().sort((a, b) => a.name.localeCompare(b.name))) {
      rows.push({ kind: "language", language, write: fenceNameOf(language), via: null });
    }
    return rows;
  }
  if (PLAIN_WORDS.some((word) => word.startsWith(q))) rows.push({ kind: "plain" });
  const found: { language: CodeLanguage; score: number; via: string | null }[] = [];
  for (const language of allLanguages()) {
    const hit = rank(language, q);
    if (hit) found.push({ language, ...hit });
  }
  found.sort((a, b) => a.score - b.score || a.language.name.localeCompare(b.language.name));
  let exact = false;
  for (const { language, score, via } of found.slice(0, limit)) {
    const named = score === 0;
    if (named) exact = true;
    rows.push({
      kind: "language",
      language,
      write: named ? typed : fenceNameOf(language),
      via,
    });
  }
  if (!exact) rows.push({ kind: "custom", write: typed });
  return rows;
}

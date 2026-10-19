// Highlighting for the config files `language-data` does not know: dotenv and ignore files, and the names that
// are INI files without saying so. (TOML, YAML and `.properties` come from `language-data`.)

import { StreamLanguage, type StreamParser } from "@codemirror/language";

type EnvState = { value: boolean };

/** `KEY=value`, `export KEY=value`, `# comment`, quoted values, `${VAR}`. */
const dotenv: StreamParser<EnvState> = {
  name: "dotenv",
  startState: () => ({ value: false }),
  token(stream, state) {
    if (stream.sol()) state.value = false;
    if (!state.value) {
      if (stream.eatSpace()) return null;
      if (stream.match(/^#.*/)) return "comment";
      if (stream.match(/^export(?=\s)/)) return "keyword";
      if (stream.match(/^[\p{L}\p{N}_.-]+/u)) return "propertyName";
      if (stream.eat("=")) {
        state.value = true;
        return "operator";
      }
      stream.next();
      return null;
    }
    if (stream.eatSpace()) return null;
    if (stream.peek() === "#" && /\s/.test(stream.string.charAt(stream.pos - 1))) {
      stream.skipToEnd();
      return "comment";
    }
    if (stream.match(/^"(?:[^"\\]|\\.)*"?/)) return "string";
    if (stream.match(/^'[^']*'?/)) return "string";
    if (stream.match(/^\$\{[^}]*\}?|^\$[\p{L}_][\p{L}\p{N}_]*/u)) return "variableName";
    stream.next();
    return null;
  },
};

/** Ignore files: `# comment`, `!` negation, wildcards. */
const ignore: StreamParser<null> = {
  name: "ignore",
  startState: () => null,
  token(stream) {
    if (stream.sol() && stream.match(/^\s*#.*/)) return "comment";
    if (stream.sol() && stream.eat("!")) return "operator";
    if (stream.match(/^\*\*|^[*?]|^\[[^\]]*\]/)) return "keyword";
    if (stream.eat("/")) return "punctuation";
    stream.next();
    return "string";
  },
};

const DOTENV = StreamLanguage.define(dotenv);
const IGNORE = StreamLanguage.define(ignore);

/** The names that are INI files (their language comes from `language-data`'s INI entry). */
const INI_NAMES =
  /^(\.editorconfig|\.gitconfig|\.gitmodules|\.npmrc|\.pypirc|\.flake8|\.pylintrc)$|\.(cfg|conf)$/i;

/** Our own language for a file name, or a file name `language-data` knows an INI file by; none = look it up as usual. */
export function configLanguage(name: string): StreamLanguage<unknown> | string | null {
  const lower = name.toLowerCase();
  if (lower === ".env" || lower.startsWith(".env.") || lower.endsWith(".env")) return DOTENV;
  if (lower.startsWith(".") && lower.endsWith("ignore")) return IGNORE;
  if (INI_NAMES.test(lower)) return "config.ini";
  return null;
}

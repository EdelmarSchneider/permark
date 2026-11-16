// Inline math in the editor's reading, by the rules View's renderer applies (comrak `math_dollars` + `math_code`, after
// pandoc, + `math_latex`): `$x$` (no space after the opening `$` nor before the closing one, no digit right after it,
// `\$` inside is not a closing one), `$$x$$`, code math `` $`x`$ ``, and the LaTeX delimiters `\(x\)` | `\[x\]`
// (closed by the first backslash right before the matching bracket; never empty). `\[x\]` is a formula only on lines
// of its own in a paragraph: its `\[` first on its line with nothing after it there (or the formula is the whole
// paragraph), its `\]` ending its line; elsewhere (`see \[1\]`, as HTML converters write brackets) it is escaped
// brackets, as View reads it. A run of three or more `$` is text. So `$5 and $6` stays text, as in View. Without these
// rules the editor would read `$a_b_c$` as emphasis (and `\(x\)` as escaped brackets) and change what the file says.
// The formula becomes an `inlineSource` mdast node holding its exact source; what the person edits is the TeX inside.

import type {
  Code,
  Construct,
  Effects,
  Extension,
  State,
  TokenizeContext,
} from "micromark-util-types";
import type { Extension as FromMarkdownExtension } from "mdast-util-from-markdown";

declare module "micromark-util-types" {
  interface TokenTypeMap {
    pmInlineMath: "pmInlineMath";
    pmInlineMathText: "pmInlineMathText";
  }
}

const DOLLAR = 36;
const BACKTICK = 96;
const BACKSLASH = 92;
const PAREN_OPEN = 40;
const PAREN_CLOSE = 41;
const BRACKET_OPEN = 91;
const BRACKET_CLOSE = 93;
const MATH = "pmInlineMath";
const MATH_TEXT = "pmInlineMathText";

/** Whitespace as the renderer counts it: space, tab, line endings, vertical tab, form feed. */
function isSpace(code: Code): boolean {
  return code !== null && (code < 0 || code === 32 || code === 11 || code === 12);
}

function isDigit(code: Code): boolean {
  return code !== null && code >= 48 && code <= 57;
}

function isLineEnding(code: Code): boolean {
  return code !== null && code < -2;
}

/** Space or tab (a tab comes as -2 and the virtual spaces after it as -1). */
function isBlank(code: Code): boolean {
  return code === 32 || code === -1 || code === -2;
}

function tokenizeMath(this: TokenizeContext, effects: Effects, ok: State, nok: State): State {
  let opening = 0;
  /** The code before the one at hand (inside the formula). */
  let last: Code = null;
  /** Codes after the opening of code math (its content and closing backtick). */
  let codeLength = 0;
  let inText = false;

  const take = (code: Code): void => {
    if (!inText) {
      effects.enter(MATH_TEXT);
      inText = true;
    }
    effects.consume(code);
    last = code;
  };
  const lineEnding = (code: Code): void => {
    if (inText) {
      effects.exit(MATH_TEXT);
      inText = false;
    }
    effects.enter("lineEnding");
    effects.consume(code);
    effects.exit("lineEnding");
    last = code;
  };
  const done: State = (code) => {
    if (inText) effects.exit(MATH_TEXT);
    effects.exit(MATH);
    return ok(code);
  };

  const start: State = (code) => {
    effects.enter(MATH);
    return open(code);
  };
  const open: State = (code) => {
    if (code === DOLLAR) {
      take(code);
      opening += 1;
      return open;
    }
    if (opening > 2) return nok(code);
    if (opening === 1 && code === BACKTICK) {
      take(code);
      return inCode;
    }
    if (opening === 2) return inDisplay(code);
    return isSpace(code) ? nok(code) : inInline(code);
  };

  const inInline: State = (code) => {
    if (code === null) return nok(code);
    if (isLineEnding(code)) {
      lineEnding(code);
      return inInline;
    }
    if (code === DOLLAR) {
      if (isSpace(last)) return nok(code);
      if (last === BACKSLASH) {
        take(code);
        return inInline;
      }
      take(code);
      return afterInline;
    }
    take(code);
    return inInline;
  };
  const afterInline: State = (code) => (isDigit(code) ? nok(code) : done(code));

  const inDisplay: State = (code) => {
    if (code === null) return nok(code);
    if (isLineEnding(code)) {
      lineEnding(code);
      return inDisplay;
    }
    take(code);
    return code === DOLLAR ? displayDollar : inDisplay;
  };
  // one `$` inside display math is part of it; two close it
  const displayDollar: State = (code) => {
    if (code !== DOLLAR) return inDisplay(code);
    take(code);
    return done;
  };

  const inCode: State = (code) => {
    if (code === null) return nok(code);
    if (code === DOLLAR && last === BACKTICK) {
      // the first `` `$ `` closes it; nothing between the backticks: it is text
      if (codeLength < 2) return nok(code);
      take(code);
      return done;
    }
    if (isLineEnding(code)) lineEnding(code);
    else take(code);
    codeLength += 1;
    return inCode;
  };

  return start;
}

/**
 * A `$` right after another one starts nothing (it belongs to the run before it), unless that one closed a formula or
 * was escaped.
 */
function previous(this: TokenizeContext, code: Code): boolean {
  if (code !== DOLLAR) return true;
  const type = this.events.at(-1)?.[1].type;
  return type === MATH || type === "characterEscape";
}

const mathConstruct: Construct = { name: "pmInlineMath", tokenize: tokenizeMath, previous };

/** The bracket that closes a LaTeX delimiter opened by `code` (`(` -> `)`, `[` -> `]`), else null. */
function latexCloser(code: Code): number | null {
  if (code === PAREN_OPEN) return PAREN_CLOSE;
  if (code === BRACKET_OPEN) return BRACKET_CLOSE;
  return null;
}

/** Spaces | tabs up to a line ending, or up to the end of the text only: checked, never taken. */
function blankTo(lineEnds: boolean): Construct {
  return {
    partial: true,
    tokenize(effects, ok, nok) {
      const rest: State = (code) => {
        if (code === null || (lineEnds && isLineEnding(code))) return ok(code);
        if (!isBlank(code)) return nok(code);
        effects.enter("whitespace");
        effects.consume(code);
        effects.exit("whitespace");
        return rest;
      };
      return rest;
    },
  };
}

const blankLine = blankTo(true);
const blankText = blankTo(false);

/** At the start of a line of the text: after a line ending, or after the indent of a continuation line. */
function atLineStart(context: TokenizeContext): boolean {
  if (context.previous === null || isLineEnding(context.previous)) return true;
  const last = context.events.at(-1);
  return last?.[0] === "exit" && last[1].type === "linePrefix";
}

/** `\(x\)` | `\[x\]`: tried before a backslash escape, as the renderer tries it. */
function tokenizeLatexMath(this: TokenizeContext, effects: Effects, ok: State, nok: State): State {
  /** `\[` opens the text (a formula sharing its opening line with TeX must then be the whole paragraph). */
  const textStart = this.previous === null;
  const lineStart = atLineStart(this);
  /** Nothing follows the opening `\[` on its line. */
  let ownLine = false;
  let closer: number | null = null;
  /** Codes of the TeX before the backslash at hand (an empty formula is text). */
  let length = 0;
  let inText = false;

  const take = (code: Code): void => {
    if (!inText) {
      effects.enter(MATH_TEXT);
      inText = true;
    }
    effects.consume(code);
  };

  const start: State = (code) => {
    effects.enter(MATH);
    take(code);
    return open;
  };
  const open: State = (code) => {
    closer = latexCloser(code);
    if (closer === null || (closer === BRACKET_CLOSE && !lineStart)) return nok(code);
    take(code);
    if (closer === PAREN_CLOSE) return inside;
    return effects.check(blankLine, alone, textStart ? inside : nok);
  };
  const alone: State = (code) => {
    ownLine = true;
    return inside(code);
  };
  const inside: State = (code) => {
    if (code === null) return nok(code);
    if (isLineEnding(code)) {
      if (inText) {
        effects.exit(MATH_TEXT);
        inText = false;
      }
      effects.enter("lineEnding");
      effects.consume(code);
      effects.exit("lineEnding");
      length += 1;
      return inside;
    }
    take(code);
    if (code === BACKSLASH) return afterBackslash;
    length += 1;
    return inside;
  };
  const afterBackslash: State = (code) => {
    if (code !== closer) {
      // the backslash was TeX
      length += 1;
      return inside(code);
    }
    if (length === 0) return nok(code);
    take(code);
    if (inText) effects.exit(MATH_TEXT);
    effects.exit(MATH);
    if (closer === PAREN_CLOSE) return ok;
    // the closing `\]` ends its line; when TeX follows the opening `\[` on its line, it ends the text
    return effects.check(ownLine ? blankLine : blankText, ok, nok);
  };

  return start;
}

/** Text with its backslash escapes undone (`\_` -> `_`). */
function unescaped(text: string): string {
  return text.replace(/\\([!-/:-@[-`{-~])/g, "$1");
}

const latexMathConstruct: Construct = { name: "pmLatexMath", tokenize: tokenizeLatexMath };

export const mathSyntax: Extension = {
  text: { [DOLLAR]: mathConstruct, [BACKSLASH]: latexMathConstruct },
};

export const mathFromMarkdown: FromMarkdownExtension = {
  enter: {
    [MATH](token) {
      const value = this.sliceSerialize(token);
      if (value.startsWith("\\[") && this.stack.at(-1)?.type !== "paragraph") {
        // display math stands in a paragraph only: in a heading, a table cell or emphasis its brackets are escaped
        // text, as View reads them
        this.enter({ type: "text", value: `[${unescaped(value.slice(2, -2))}]` }, token);
        return;
      }
      this.enter({ type: "inlineSource", kind: "math", value }, token);
    },
  },
  exit: {
    [MATH](token) {
      this.exit(token);
    },
  },
};

/** Each opening delimiter with its closing one, longest first (`` $` `` always opens code math). */
const DELIMITERS: readonly (readonly [string, string])[] = [
  ["$`", "`$"],
  ["$$", "$$"],
  ["$", "$"],
  ["\\(", "\\)"],
  ["\\[", "\\]"],
];

/** Delimiters of a formula on a line of its own. */
const DISPLAY = new Set(["$$", "\\["]);

/** A formula's source as its delimiters and the TeX between them (`text`); null when `source` is not one. */
export function mathParts(source: string): { open: string; text: string; close: string } | null {
  // `` $` `` always opens code math: when it is not closed, the `$` is text (never `$` math)
  const found = DELIMITERS.find(([open]) => source.startsWith(open));
  if (found === undefined) return null;
  const [open, close] = found;
  if (source.length <= open.length + close.length || !source.endsWith(close)) return null;
  return { open, text: source.slice(open.length, source.length - close.length), close };
}

/**
 * The TeX View draws for a formula: line endings as spaces; code math loses one space on each side. Display and LaTeX
 * delimited math keep the TeX as written.
 */
function literal(open: string, tex: string): string {
  if (DISPLAY.has(open) || open === "\\(") return tex;
  const flat = tex.replace(/\r\n|\r|\n/g, " ");
  return open === "$`" && /^ .*\S.* $/s.test(flat) ? flat.slice(1, -1) : flat;
}

/** View's element for a formula (its TeX as text, never markup): what View's math embed draws in. */
export function mathElement(source: string): HTMLElement {
  const parts = mathParts(source);
  const code = parts?.open === "$`";
  const element = document.createElement(code ? "code" : "span");
  element.dataset.mathStyle = DISPLAY.has(parts?.open ?? "") ? "display" : "inline";
  element.textContent = parts === null ? source : literal(parts.open, parts.text);
  return element;
}

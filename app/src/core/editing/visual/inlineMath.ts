// Inline math in the editor's reading, by the rules View's renderer applies (comrak `math_dollars` + `math_code`, after
// pandoc): `$x$` (no space after the opening `$` nor before the closing one, no digit right after it, `\$` inside is
// not a closing one), `$$x$$`, and code math `` $`x`$ ``. A run of three or more `$` is text. So `$5 and $6` stays
// text, as in View. Without these rules the editor would read `$a_b_c$` as emphasis and change what the file says.
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

export const mathSyntax: Extension = { text: { [DOLLAR]: mathConstruct } };

export const mathFromMarkdown: FromMarkdownExtension = {
  enter: {
    [MATH](token) {
      this.enter({ type: "inlineSource", kind: "math", value: this.sliceSerialize(token) }, token);
    },
  },
  exit: {
    [MATH](token) {
      this.exit(token);
    },
  },
};

/** A formula's source as its delimiters and the TeX between them (`text`); null when `source` is not one. */
export function mathParts(source: string): { open: string; text: string; close: string } | null {
  // `` $` `` always opens code math: when it is not closed, the `$` is text (never `$` math)
  const open = ["$`", "$$", "$"].find((delimiter) => source.startsWith(delimiter));
  if (open === undefined) return null;
  const close = open === "$`" ? "`$" : open;
  if (source.length <= open.length + close.length || !source.endsWith(close)) return null;
  return { open, text: source.slice(open.length, source.length - close.length), close };
}

/** The TeX View draws for a formula: line endings as spaces; code math loses one space on each side. */
function literal(open: string, tex: string): string {
  if (open === "$$") return tex;
  const flat = tex.replace(/\r\n|\r|\n/g, " ");
  return open === "$`" && /^ .*\S.* $/s.test(flat) ? flat.slice(1, -1) : flat;
}

/** View's element for a formula (its TeX as text, never markup): what View's math embed draws in. */
export function mathElement(source: string): HTMLElement {
  const parts = mathParts(source);
  const code = parts?.open === "$`";
  const element = document.createElement(code ? "code" : "span");
  element.dataset.mathStyle = parts?.open === "$$" ? "display" : "inline";
  element.textContent = parts === null ? source : literal(parts.open, parts.text);
  return element;
}

// The CSS a drawing brings (its `<style>`, its `style` attributes, presentation attributes such as `fill`), read with
// the page's own tokenizing rules (escapes, comments, strings, url tokens, nested blocks), so a spelling the page
// reads as `position` or `url(` reads the same here. Text that is kept stays byte for byte.

type Kind =
  "space" | "comment" | "name" | "function" | "at" | "string" | "url" | "badUrl" | "delim";
/** `text`: a name, function or at-keyword decoded and ASCII-lowercased; a string or url decoded; a delim's char. */
type Token = { kind: Kind; start: number; end: number; text: string };

const SPACE = /[ \t\n\r\f]/;
const NEWLINE = /[\n\r\f]/;
const NAME_CHAR = /[\w\u0080-\uffff-]/;
const HEX = /^[0-9a-f]{1,6}/i;
const NON_PRINTABLE = /[\u0000-\u0008\u000b\u000e-\u001f\u007f]/;
const QUOTED_ARGUMENT = /[ \t\n\r\f]*["']/y;
/** Functions that load a picture from a bare string, with no `url(` to check. */
const LOADS_FROM_STRING = new Set(["image-set", "-webkit-image-set", "image"]);
/**
 * Positions that leave a part where the flow puts it, inside its figure, whose own overflow clips whatever else the
 * CSS does to it (transforms, margins, shadows). Out of the flow (`fixed`, `sticky`, `absolute`, a value read from
 * elsewhere: `var()`, `inherit`) nothing between it and the window has to clip it; `relative` would let a `z-index`
 * lift it over the page's own overlays.
 */
const IN_FLOW = new Set(["static", "initial", "unset"]);
/**
 * Keyframe names are the page's, not the drawing's: a drawing's `@keyframes pm-fade` would replace the app's (its own
 * `pm-`, Svelte's scoped `svelte-`, the editors' `cm-blink` | `ProseMirror-cursor-blink`).
 */
const PAGE_FRAMES = /^(?:pm|svelte|cm|prosemirror)-/;
/** Combinators after the root's part that reach its siblings, out of the drawing (`|` starts the column one). */
const SIBLING = new Set(["+", "~", "|"]);

const lower = (text: string): string => text.replace(/[A-Z]+/g, (upper) => upper.toLowerCase());
const significant = (token: Token): boolean => token.kind !== "space" && token.kind !== "comment";
const isDelim = (token: Token | undefined, char: string): boolean =>
  token?.kind === "delim" && token.text === char;

/** The character a `\` at `at` escapes and where the escape ends; null when a line break follows (no escape). */
function escapeAt(css: string, at: number): { char: string; end: number } | null {
  const next = css.codePointAt(at + 1);
  if (next === undefined) return { char: "�", end: at + 1 };
  if (NEWLINE.test(String.fromCodePoint(next))) return null;
  const hex = HEX.exec(css.slice(at + 1, at + 7));
  if (hex === null) {
    const char = String.fromCodePoint(next);
    return { char, end: at + 1 + char.length };
  }
  let end = at + 1 + hex[0].length;
  if (css.startsWith("\r\n", end)) end += 2;
  else if (SPACE.test(css[end] ?? "")) end += 1;
  const code = Number.parseInt(hex[0], 16);
  const valid = code > 0 && code <= 0x10ffff && (code < 0xd800 || code > 0xdfff);
  return { char: valid ? String.fromCodePoint(code) : "�", end };
}

/** A run of name characters and escapes from `at` (numbers and dimensions read as names too: never compared). */
function nameAt(css: string, at: number): { name: string; end: number } | null {
  let name = "";
  let i = at;
  while (i < css.length) {
    const char = css[i] ?? "";
    if (char === "\\") {
      const escape = escapeAt(css, i);
      if (escape === null) break;
      name += escape.char;
      i = escape.end;
    } else if (NAME_CHAR.test(char)) {
      name += char;
      i += 1;
    } else break;
  }
  return i === at ? null : { name, end: i };
}

/** A string from its opening quote; a line break ends it unread (the page drops such a declaration). */
function stringAt(css: string, at: number): { value: string; end: number } {
  const quote = css[at];
  let value = "";
  let i = at + 1;
  while (i < css.length) {
    const char = css[i] ?? "";
    if (char === quote) return { value, end: i + 1 };
    if (NEWLINE.test(char)) return { value, end: i };
    if (char === "\\") {
      const escape = escapeAt(css, i);
      if (escape === null) {
        // an escaped line break continues the string
        i += css.startsWith("\r\n", i + 1) ? 3 : 2;
      } else if (i + 1 >= css.length) {
        i += 1;
      } else {
        value += escape.char;
        i = escape.end;
      }
      continue;
    }
    value += char;
    i += 1;
  }
  return { value, end: i };
}

/** The rest of a malformed url token: up to its `)`, an escaped `)` included. */
function badUrlEnd(css: string, at: number): number {
  let i = at;
  while (i < css.length) {
    if (css[i] === ")") return i + 1;
    i += css[i] === "\\" ? 2 : 1;
  }
  return css.length;
}

/** An unquoted `url(` token from just after its `(`. */
function urlAt(css: string, at: number): { kind: "url" | "badUrl"; value: string; end: number } {
  let i = at;
  while (SPACE.test(css[i] ?? "")) i += 1;
  let value = "";
  while (i < css.length) {
    const char = css[i] ?? "";
    if (char === ")") return { kind: "url", value, end: i + 1 };
    if (SPACE.test(char)) {
      while (SPACE.test(css[i] ?? "")) i += 1;
      if (i >= css.length) break;
      if (css[i] === ")") return { kind: "url", value, end: i + 1 };
      return { kind: "badUrl", value, end: badUrlEnd(css, i) };
    }
    const escape = char === "\\" ? escapeAt(css, i) : undefined;
    if (
      char === '"' ||
      char === "'" ||
      char === "(" ||
      NON_PRINTABLE.test(char) ||
      escape === null
    ) {
      return { kind: "badUrl", value, end: badUrlEnd(css, i) };
    }
    value += escape?.char ?? char;
    i = escape?.end ?? i + 1;
  }
  return { kind: "url", value, end: i };
}

function tokens(css: string): Token[] {
  const list: Token[] = [];
  let i = 0;
  while (i < css.length) {
    const start = i;
    const char = css[i] ?? "";
    if (css.startsWith("/*", i)) {
      const close = css.indexOf("*/", i + 2);
      i = close < 0 ? css.length : close + 2;
      list.push({ kind: "comment", start, end: i, text: "" });
    } else if (SPACE.test(char)) {
      while (SPACE.test(css[i] ?? "")) i += 1;
      list.push({ kind: "space", start, end: i, text: "" });
    } else if (char === '"' || char === "'") {
      const string = stringAt(css, i);
      i = string.end;
      list.push({ kind: "string", start, end: i, text: string.value });
    } else {
      const at = char === "@" ? nameAt(css, i + 1) : null;
      const name = at ?? nameAt(css, i);
      if (name === null) {
        i += 1;
        list.push({ kind: "delim", start, end: i, text: char });
      } else if (at !== null) {
        i = at.end;
        list.push({ kind: "at", start, end: i, text: lower(at.name) });
      } else if (css[name.end] === "(") {
        const fn = lower(name.name);
        QUOTED_ARGUMENT.lastIndex = name.end + 1;
        if (fn === "url" && !QUOTED_ARGUMENT.test(css)) {
          const url = urlAt(css, name.end + 1);
          i = url.end;
          list.push({ kind: url.kind, start, end: i, text: url.value });
        } else {
          i = name.end + 1;
          list.push({ kind: "function", start, end: i, text: fn });
        }
      } else {
        i = name.end;
        list.push({ kind: "name", start, end: i, text: lower(name.name) });
      }
    }
  }
  return list;
}

/**
 * Whether the tokens name anything outside the drawing: a `url(...)` | `src(...)` that is not a `#id` of the page,
 * a picture from a string (`image-set("x.png")`), an `@import`.
 */
function loads(list: readonly Token[]): boolean {
  return list.some((token, at) => {
    switch (token.kind) {
      case "url":
        return !token.text.startsWith("#");
      case "badUrl":
        return true;
      case "at":
        return token.text === "import";
      case "function": {
        if (LOADS_FROM_STRING.has(token.text)) return true;
        if (token.text !== "url" && token.text !== "src") return false;
        let argument: Token | undefined;
        for (let next = at + 1; argument === undefined && next < list.length; next += 1) {
          const other = list[next];
          if (other !== undefined && significant(other)) argument = other;
        }
        return argument?.kind !== "string" || !argument.text.startsWith("#");
      }
      default:
        return false;
    }
  });
}

/** A `position` value that keeps the element in the flow, `!important` or not. */
function keepsInFlow(value: readonly Token[]): boolean {
  const [word, bang, important, ...more] = value;
  if (word === undefined) return true;
  if (word.kind !== "name" || !IN_FLOW.has(word.text)) return false;
  if (bang === undefined) return true;
  return (
    more.length === 0 &&
    isDelim(bang, "!") &&
    important?.kind === "name" &&
    important.text === "important"
  );
}

/**
 * What a block holds: rules only (a style sheet, a group rule in it), declarations and nested rules (a style rule),
 * keyframes, or the inside of a value's brackets.
 */
type Holds = "sheet" | "style" | "frames" | "value";
/** A style sheet's own reading: the id of the drawing's root, which every selector must start with. */
type Sheet = { root: string | null };

/**
 * A declaration or an at-rule without a block (`;`, `}` or the end closes it) that must go. In a style sheet no such
 * at-rule stays (`@import`, `@namespace`, `@layer a, b;` order the whole page), nor a statement among rules, which is
 * no rule at all.
 */
function dropsStatement(list: readonly Token[], holds: Holds, sheet: Sheet | null): boolean {
  const [name, colon, ...value] = list.filter(significant);
  if (name === undefined) return false;
  if (sheet !== null && (holds === "sheet" || name.kind === "at")) return true;
  if (loads(list)) return true;
  return (
    name.kind === "name" && name.text === "position" && isDelim(colon, ":") && !keepsInFlow(value)
  );
}

/**
 * Whether a selector matches only the drawing's root or parts inside it: it starts with `#root` as written (another
 * spelling may name another element: never kept), and no sibling combinator follows the compound holding it.
 */
function startsAtRoot(css: string, selector: readonly Token[], root: string): boolean {
  const at = selector.findIndex(significant);
  const hash = selector[at];
  const name = selector[at + 1];
  if (!isDelim(hash, "#") || name?.kind !== "name" || name.start !== hash?.end) return false;
  if (css.slice(name.start, name.end) !== root) return false;
  const closers: string[] = [];
  let gap = false;
  for (const token of selector.slice(at + 2)) {
    const closer = closerOf(token);
    if (closers.length > 0) {
      if (closer !== null) closers.push(closer);
      else if (isDelim(token, closers.at(-1) ?? "")) closers.pop();
    } else if (!significant(token)) {
      gap = true;
    } else if (token.kind === "delim" && (SIBLING.has(token.text) || token.text === ">")) {
      return token.text === ">";
    } else if (gap) {
      // a descendant combinator
      return true;
    } else if (closer !== null) closers.push(closer);
  }
  return true;
}

/**
 * The cuts that leave in a style rule's selector list only the selectors starting at the drawing's root (each with
 * its comma); null when none does: the rule goes whole.
 */
function rootSelectors(
  css: string,
  prelude: readonly Token[],
  root: string,
): [number, number][] | null {
  const selectors: Token[][] = [[]];
  const commas: Token[] = [];
  const closers: string[] = [];
  for (const token of prelude) {
    const closer = closerOf(token);
    if (closer !== null) closers.push(closer);
    else if (closers.length > 0 && isDelim(token, closers.at(-1) ?? "")) closers.pop();
    else if (closers.length === 0 && isDelim(token, ",")) {
      commas.push(token);
      selectors.push([]);
      continue;
    }
    selectors.at(-1)?.push(token);
  }
  const inside = selectors.map((selector) => startsAtRoot(css, selector, root));
  const first = inside.indexOf(true);
  const kept = selectors[first]?.find(significant);
  if (kept === undefined) return null;
  const cuts: [number, number][] = [];
  // the selectors before the first kept one go with the comma after each
  const lead = selectors[0]?.find(significant)?.start ?? commas[0]?.start ?? kept.start;
  if (first > 0) cuts.push([lead, kept.start]);
  // the later ones with the comma before each
  for (let k = first + 1; k < selectors.length; k += 1) {
    const comma = commas[k - 1];
    if (inside[k] === true || comma === undefined) continue;
    cuts.push([comma.start, selectors[k]?.filter(significant).at(-1)?.end ?? comma.end]);
  }
  return cuts;
}

/**
 * What the block a `{` opens after `prelude` holds, with the cuts its prelude takes; null when the rule goes whole: its
 * prelude loads, or (in a style sheet) it reaches outside the drawing. A sheet keeps style rules at the drawing's root,
 * `@media` | `@supports` around rules checked the same way, and keyframes under a name of its own; any other at-rule
 * (`@font-face`, `@page`, `@property`, `@layer`, `@container`, `@scope`, ...) names or reaches what is the page's.
 */
function opening(
  css: string,
  prelude: readonly Token[],
  holds: Holds,
  sheet: Sheet | null,
): { holds: Holds; cuts: [number, number][] } | null {
  if (loads(prelude)) return null;
  // keyframe selectors (`from`, `50%`) name no element
  if (sheet === null || holds === "frames") return { holds: "style", cuts: [] };
  const [first, name] = prelude.filter(significant);
  if (first?.kind !== "at") {
    const cuts = sheet.root === null ? null : rootSelectors(css, prelude, sheet.root);
    return cuts === null ? null : { holds: "style", cuts };
  }
  switch (first.text) {
    case "media":
    case "supports":
      return { holds, cuts: [] };
    case "keyframes":
    case "-webkit-keyframes":
      return (name?.kind === "name" || name?.kind === "string") &&
        !PAGE_FRAMES.test(lower(name.text))
        ? { holds: "frames", cuts: [] }
        : null;
    default:
      return null;
  }
}

/** The bracket a token opens, by the char that closes it. */
function closerOf(token: Token): string | null {
  if (token.kind === "function" || isDelim(token, "(")) return ")";
  if (isDelim(token, "[")) return "]";
  if (isDelim(token, "{")) return "}";
  return null;
}

/** Where the block opened by the `{` at `open` closes (past the end when it never does). */
function blockEnd(list: readonly Token[], open: number): number {
  const closers: string[] = [];
  for (let i = open; i < list.length; i += 1) {
    const token = list[i];
    if (token === undefined) break;
    const closer = closerOf(token);
    if (closer !== null) closers.push(closer);
    else if (isDelim(token, closers.at(-1) ?? "")) {
      closers.pop();
      if (closers.length === 0) return i;
    }
  }
  return list.length;
}

/**
 * `css` without the declarations that would take the drawing out of the flow or load something, and without a rule
 * whose prelude loads (`@supports (x: url(…))`); a style sheet (`sheet`) also without what reaches outside the drawing
 * (see `opening`). Brackets nest as the page nests them: inside a function, `[…]` or a block within one, a `;` | `{` |
 * `}` ends no declaration; a closer that matches no open bracket is a plain token.
 */
function clean(css: string, sheet: Sheet | null): string {
  const list = tokens(css);
  const cuts: [number, number][] = [];
  const outer: Holds = sheet === null ? "style" : "sheet";
  const open: { close: string; holds: Holds }[] = [];
  let from = 0;
  const cutTo = (end: number): void => {
    const first = list[from];
    if (first !== undefined && first.start < end) cuts.push([first.start, end]);
  };
  let i = 0;
  while (i < list.length) {
    const token = list[i];
    if (token === undefined) break;
    const top = open.at(-1);
    const holds = top?.holds ?? outer;
    const amongRules = holds !== "value";
    const closer = closerOf(token);
    if (closer === "}" && amongRules) {
      const block = opening(css, list.slice(from, i), holds, sheet);
      if (block === null) {
        const end = blockEnd(list, i);
        cutTo(list[end]?.end ?? css.length);
        i = end + 1;
        from = i;
        continue;
      }
      cuts.push(...block.cuts);
      open.push({ close: closer, holds: block.holds });
      from = i + 1;
    } else if (closer !== null) {
      open.push({ close: closer, holds: "value" });
    } else if (amongRules && (isDelim(token, ";") || isDelim(token, "}"))) {
      const statement = list.slice(from, i);
      if (dropsStatement(statement, holds, sheet)) {
        // the `;` goes with its declaration; the `}` closes the block around it and stays
        const last = statement.filter(significant).at(-1);
        cutTo(isDelim(token, ";") ? token.end : (last?.end ?? token.start));
      }
      if (isDelim(token, "}") && top !== undefined) open.pop();
      from = i + 1;
    } else if (top !== undefined && isDelim(token, top.close)) {
      open.pop();
    }
    i += 1;
  }
  if (dropsStatement(list.slice(from), open.at(-1)?.holds ?? outer, sheet)) cutTo(css.length);
  if (cuts.length === 0) return css;
  let kept = "";
  let at = 0;
  for (const [start, end] of cuts) {
    kept += css.slice(at, start);
    at = end;
  }
  return kept + css.slice(at);
}

/** A `style` attribute without the declarations that would take a part out of the flow or load something. */
export function cleanCss(css: string): string {
  return clean(css, null);
}

/**
 * A drawing's style sheet as `clean` keeps it, `root` being the id the page reads on the drawing's root (null: none;
 * no rule stays). The page applies a sheet to the whole window, wherever the `<style>` sits: only rules at that root
 * reach nothing else.
 */
export function cleanSheet(css: string, root: string | null): string {
  const sheet = { root };
  const kept = clean(css, sheet);
  // text joined across a cut is read again: what it holds must be what the first reading checked
  return kept === css || clean(kept, sheet) === kept ? kept : "";
}

/** Whether an attribute value the page may read as CSS (`fill`, `filter`, `cursor`, ...) loads anything. */
export function loadsOutside(value: string): boolean {
  // a url token and every function need a literal `(`; most attribute values (numbers, path data) have none
  return value.includes("(") && loads(tokens(value));
}

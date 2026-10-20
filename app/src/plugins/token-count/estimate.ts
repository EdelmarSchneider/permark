// Mirror of `permark_core::tokens` and the crate's `estimate.rs`: the same classes, the same integer weights (milli-tokens),
// the same results on every text (shared fixtures: `crates/plugins/token-count/fixtures/estimate.json`). It gives the
// number shown at once while an exact count is being worked out (marked approximate) and the estimates themselves.

import type { TokenizerId } from "./tokenizers";

type Features = {
  space: number;
  tab: number;
  newline: number;
  lower: number;
  upper: number;
  digit: number;
  latinExt: number;
  cjk: number;
  greekCyr: number;
  letterOther: number;
  emoji: number;
  symbol: number;
  /** ASCII punctuation not right after ASCII punctuation. */
  punct: number;
  /** ASCII punctuation right after ASCII punctuation (`":"`, `},{`). */
  punctJoin: number;
  word: number;
  excess: number;
};

type Weights = Features;
type Class = Exclude<keyof Features, "word" | "excess" | "punctJoin">;

/** A word has this many characters before the rest count as `excess`. */
const EXCESS_FROM = 6;

const between = (cp: number, from: number, to: number): boolean => cp >= from && cp <= to;

/** The class of a code point (first match, as in the Rust table). */
export function classOf(cp: number): Class {
  if (cp === 0x09) return "tab";
  if (
    cp === 0x20 ||
    cp === 0xa0 ||
    cp === 0x1680 ||
    between(cp, 0x2000, 0x200a) ||
    cp === 0x202f ||
    cp === 0x205f ||
    cp === 0x3000
  )
    return "space";
  if (between(cp, 0x0a, 0x0d) || cp === 0x85 || cp === 0x2028 || cp === 0x2029) return "newline";
  if (between(cp, 0x30, 0x39)) return "digit";
  if (between(cp, 0x41, 0x5a)) return "upper";
  if (between(cp, 0x61, 0x7a)) return "lower";
  if (cp <= 0x7f) return "punct";
  if (between(cp, 0x80, 0xbf) || cp === 0xd7 || cp === 0xf7) return "symbol";
  if (between(cp, 0xc0, 0x36f) || between(cp, 0x1e00, 0x1eff)) return "latinExt";
  if (
    between(cp, 0x2e80, 0x2fdf) ||
    between(cp, 0x3040, 0x30ff) ||
    between(cp, 0x3100, 0x312f) ||
    between(cp, 0x3190, 0x31ff) ||
    between(cp, 0x3400, 0x4dbf) ||
    between(cp, 0x4e00, 0x9fff) ||
    between(cp, 0xac00, 0xd7af) ||
    between(cp, 0xf900, 0xfaff) ||
    between(cp, 0x20000, 0x3ffff)
  )
    return "cjk";
  if (between(cp, 0x370, 0x52f) || between(cp, 0x1f00, 0x1fff)) return "greekCyr";
  if (
    between(cp, 0x530, 0x1eff) ||
    between(cp, 0x2c00, 0x2dff) ||
    between(cp, 0xa000, 0xabff) ||
    between(cp, 0xfb1d, 0xfdff) ||
    between(cp, 0xfe70, 0xfeff)
  )
    return "letterOther";
  if (
    between(cp, 0x1f000, 0x1faff) ||
    between(cp, 0x2600, 0x27bf) ||
    between(cp, 0x2b00, 0x2bff) ||
    between(cp, 0xfe00, 0xfe0f) ||
    cp === 0x200d ||
    between(cp, 0xe0000, 0xe007f)
  )
    return "emoji";
  return "symbol";
}

function featuresOf(text: string): Features {
  const f: Features = {
    space: 0,
    tab: 0,
    newline: 0,
    lower: 0,
    upper: 0,
    digit: 0,
    latinExt: 0,
    cjk: 0,
    greekCyr: 0,
    letterOther: 0,
    emoji: 0,
    symbol: 0,
    punct: 0,
    punctJoin: 0,
    word: 0,
    excess: 0,
  };
  let run = 0;
  let previous: Class | null = null;
  for (const ch of text) {
    const cls = classOf(ch.codePointAt(0) ?? 0);
    if (cls === "punct" && previous === "punct") f.punctJoin += 1;
    else f[cls] += 1;
    previous = cls;
    if (cls === "lower" || cls === "upper" || cls === "digit" || cls === "latinExt") {
      if (run === 0) f.word += 1;
      run += 1;
      if (run > EXCESS_FROM) f.excess += 1;
    } else {
      run = 0;
    }
  }
  return f;
}

const O200K: Weights = {
  space: 118,
  tab: 118,
  newline: 387,
  lower: 0,
  upper: 81,
  digit: 515,
  latinExt: 730,
  cjk: 875,
  greekCyr: 324,
  letterOther: 403,
  emoji: 2616,
  symbol: 1224,
  punct: 514,
  punctJoin: 514,
  word: 861,
  excess: 582,
};

const CL100K: Weights = {
  space: 124,
  tab: 124,
  newline: 421,
  lower: 0,
  upper: 47,
  digit: 521,
  latinExt: 1833,
  cjk: 1338,
  greekCyr: 639,
  letterOther: 1021,
  emoji: 4357,
  symbol: 595,
  punct: 455,
  punctJoin: 455,
  word: 900,
  excess: 640,
};

const GEMINI: Weights = {
  space: 123,
  tab: 123,
  newline: 916,
  lower: 30,
  upper: 161,
  digit: 1043,
  latinExt: 532,
  cjk: 757,
  greekCyr: 300,
  letterOther: 416,
  emoji: 2706,
  symbol: 632,
  punct: 675,
  punctJoin: 675,
  word: 714,
  excess: 436,
};

const CLAUDE: Weights = {
  space: 126,
  tab: 126,
  newline: 414,
  lower: 0,
  upper: 87,
  digit: 551,
  latinExt: 781,
  cjk: 936,
  greekCyr: 347,
  letterOther: 431,
  emoji: 2799,
  symbol: 1310,
  punct: 550,
  punctJoin: 550,
  word: 921,
  excess: 623,
};

function weighted(text: string, w: Weights): number {
  const f = featuresOf(text);
  const milli =
    w.space * f.space +
    w.tab * f.tab +
    w.newline * f.newline +
    w.lower * f.lower +
    w.upper * f.upper +
    w.digit * f.digit +
    w.latinExt * f.latinExt +
    w.cjk * f.cjk +
    w.greekCyr * f.greekCyr +
    w.letterOther * f.letterOther +
    w.emoji * f.emoji +
    w.symbol * f.symbol +
    w.punct * f.punct +
    w.punctJoin * f.punctJoin +
    w.word * f.word +
    w.excess * f.excess;
  return Math.ceil(milli / 1000);
}

/** Characters divided by four, rounded up (scalar values, not UTF-16 units). */
export function chars4(text: string): number {
  let n = 0;
  for (const _ of text) n += 1;
  return Math.ceil(n / 4);
}

/** `permark_core::tokens::GENERIC`: o200k on the corpus + answers for models (tab tables, JSON, TOON). */
const GENERIC: Weights = {
  space: 121,
  tab: 593,
  newline: 712,
  lower: 0,
  upper: 12,
  digit: 497,
  latinExt: 625,
  cjk: 888,
  greekCyr: 317,
  letterOther: 396,
  emoji: 3050,
  symbol: 1026,
  punct: 639,
  punctJoin: 182,
  word: 837,
  excess: 568,
};

/** The generic estimate (`permark_core::tokens::estimate`). */
export function generic(text: string): number {
  return weighted(text, GENERIC);
}

/** Tokens of `text` by the estimator of `id`. For an exact tokenizer this is its interim estimate. */
export function estimate(id: TokenizerId, text: string): number {
  switch (id) {
    case "o200k":
      return weighted(text, O200K);
    case "cl100k":
      return weighted(text, CL100K);
    case "claude":
      return weighted(text, CLAUDE);
    case "gemini":
      return weighted(text, GEMINI);
    case "chars4":
      return chars4(text);
    default:
      return id satisfies never;
  }
}

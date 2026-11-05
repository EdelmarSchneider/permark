import { describe, expect, it } from "vitest";
import { around, suggest, tokenize, wordDiff, type Suggestion } from "./diff";

/** `doc` with every suggestion applied (they never overlap). */
function apply(doc: string, list: readonly Suggestion[]): string {
  let out = doc;
  for (const s of [...list].sort((a, b) => b.from - a.from))
    out = out.slice(0, s.from) + s.insert + out.slice(s.to);
  return out;
}

/** Suggestions for `original` found at `prefix.length` in `prefix + original + suffix`, and the doc after them. */
function run(
  original: string,
  corrected: string,
  lang?: string,
  prefix = "# Intro\n\n",
  suffix = "\n\nEnd.\n",
) {
  const doc = prefix + original + suffix;
  const list = suggest(doc, prefix.length, wordDiff(original, corrected, lang));
  return { doc, list, applied: apply(doc, list), expected: prefix + corrected + suffix };
}

describe("word suggestions per language", () => {
  const cases: [string, string, string, number][] = [
    ["en", "I has a apple and teh pear.", "I have an apple and the pear.", 2],
    ["pt-BR", "Eu nao sei se voce vem amanha.", "Eu não sei se você vem amanhã.", 3],
    ["es", "El niño estava muy cansado ayer.", "El niño estaba muy cansado ayer.", 1],
    ["de", "Ich habe das Buch gestern gelest.", "Ich habe das Buch gestern gelesen.", 1],
    ["ja", "私は学生でわありません。", "私は学生ではありません。", 1],
    ["ko", "나는 어제 학교에갔다.", "나는 어제 학교에 갔다.", 1],
    ["zh", "我今天很高心。", "我今天很高兴。", 1],
  ];
  for (const [lang, original, corrected, count] of cases) {
    it(`${lang}: offsets hit the exact document range`, () => {
      const r = run(original, corrected, lang);
      expect(r.list).toHaveLength(count);
      expect(r.applied).toBe(r.expected);
      for (const s of r.list) {
        expect(r.doc.slice(s.from, s.to)).toBe(s.original);
        expect(r.doc.slice(0, s.from).endsWith(s.before)).toBe(true);
        expect(r.doc.slice(s.to).startsWith(s.after)).toBe(true);
      }
    });
  }

  it("groups changes one space apart and keeps separate words apart", () => {
    const grouped = run("teh quik fox", "the quick fox");
    expect(grouped.list.map((s) => [s.original, s.insert])).toEqual([["teh quik", "the quick"]]);
    const apart = run("teh brown quik fox", "the brown quick fox");
    expect(apart.list.map((s) => [s.original, s.insert])).toEqual([
      ["teh", "the"],
      ["quik", "quick"],
    ]);
  });

  it("before | after carry about 40 chars of context", () => {
    const r = run(
      "Some long text before teh error and a long text after it ends.",
      "Some long text before the error and a long text after it ends.",
    );
    expect(r.list[0]?.before).toBe("Some long text before ");
    expect(r.list[0]?.after).toBe(" error and a long text after it ends.");
  });

  it("the excerpt never crosses a blank line nor starts | ends mid-word", () => {
    const doc = "# Title\n\nI has a cat.\n \nNext block";
    const at = doc.indexOf("has");
    expect(around(doc, at, at + 3)).toEqual({ before: "I ", after: " a cat." });
    const long =
      "Wordy sentence leading up to the spot where teh error sits and goes on further away from here today";
    const e = long.indexOf("teh");
    const { before, after } = around(long, e, e + 3);
    expect(before).toBe("…sentence leading up to the spot where ");
    // cut by size: back to a word end, marked
    expect(after).toBe(" error sits and goes on further away…");
    // no space to cut at (CJK): the cut stays, marked
    const ja = "あ".repeat(60) + "X" + "い".repeat(60);
    const x = ja.indexOf("X");
    expect(around(ja, x, x + 1)).toEqual({
      before: `…${"あ".repeat(40)}`,
      after: `${"い".repeat(40)}…`,
    });
  });
});

describe("whitespace", () => {
  it("ignores model reflow and keeps the original line breaks", () => {
    expect(run("one two\nthree four", "one two three four").list).toEqual([]);
    const r = run("one tow\nthree four", "one two three\n four");
    expect(r.list.map((s) => s.insert)).toEqual(["two"]);
    expect(r.applied).toBe(r.doc.replace("tow", "two"));
  });

  it("ignores added | dropped space at the ends", () => {
    expect(run("Fine text.", "  Fine text.\n").list).toEqual([]);
  });

  it("keeps a space insert | delete inside a line as an edit", () => {
    expect(run("each other", "eachother").list.map((s) => [s.original, s.insert])).toEqual([
      ["each other", "eachother"],
    ]);
    expect(run("eachother", "each other").list).toHaveLength(1);
  });
});

describe("unicode", () => {
  it("compares NFC: NFD accents stay as written", () => {
    const nfd = "Ele é café".normalize("NFD");
    expect(run(nfd, "Ele é café".normalize("NFC")).list).toEqual([]);
    const r = run(`${nfd} quente demias.`, `${"Ele é café".normalize("NFC")} quente demais.`);
    expect(r.list.map((s) => s.insert)).toEqual(["demais"]);
    expect(r.applied).toContain(nfd);
  });

  it("never splits an emoji ZWJ sequence", () => {
    const family = "👩‍👩‍👧";
    const r = run(`My ${family}famly is here.`, `My ${family}family is here.`);
    expect(r.applied).toBe(r.expected);
    for (const s of r.list) {
      const graphemes = [
        ...new Intl.Segmenter(undefined, { granularity: "grapheme" }).segment(r.doc),
      ].map((g) => g.index);
      expect(graphemes).toContain(s.from);
      expect(graphemes).toContain(s.to);
    }
  });

  it("tokenizes Japanese without spaces into words", () => {
    expect(tokenize("私は学生です。", "ja").filter((t) => t.word).length).toBeGreaterThan(2);
  });
});

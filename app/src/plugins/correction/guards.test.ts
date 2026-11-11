import { describe, expect, it } from "vitest";
import { pieces, type Piece } from "../local-ai/pieces/segment";
import { judge, type Verdict } from "./guards";

function only(doc: string): Piece {
  const [piece] = pieces(doc);
  if (piece === undefined) throw new Error(`no piece in ${doc}`);
  return piece;
}

function reason(v: Verdict): string {
  return v.kind === "discard" ? v.reason : "ok";
}

/** `doc` with the piece's suggestions applied. */
function applied(doc: string, v: Verdict): string {
  if (v.kind !== "ok") throw new Error(`discarded: ${v.reason}`);
  let out = doc;
  for (const s of [...v.suggestions].reverse())
    out = out.slice(0, s.from) + s.insert + out.slice(s.to);
  return out;
}

describe("judge", () => {
  it("accepts a fair correction and maps it onto the document", () => {
    const doc = "# Notes\n\nTeh `code` stays and [teh link](http://teh.x) is fine.\n";
    const piece = pieces(doc)[1];
    if (piece === undefined) throw new Error("no piece");
    expect(piece.text).toBe("Teh ⟦1⟧ stays and [teh link]⟦2⟧ is fine.");
    const v = judge(doc, piece, "The ⟦1⟧ stays and [the link]⟦2⟧ is fine.");
    expect(applied(doc, v)).toBe(
      "# Notes\n\nThe `code` stays and [the link](http://teh.x) is fine.\n",
    );
  });

  it("an unchanged output gives no suggestions", () => {
    const doc = "Nothing to fix here.";
    const v = judge(doc, only(doc), "Nothing to fix here.");
    expect(v).toMatchObject({ kind: "ok", suggestions: [] });
  });

  it("passes on what the structure check discards, with its own preambles and length range", () => {
    const doc = "This is fine text.";
    expect(reason(judge(doc, only(doc), "Texto corrigido: This is fine text."))).toBe("preamble");
    const long = "This sentence is complete and has a clear ending.";
    expect(reason(judge(long, only(long), "This sentence is complete."))).toBe("length");
    // Markdown the piece does not have
    expect(reason(judge(doc, only(doc), "This is <b>fine</b> text."))).toBe("added");
    expect(reason(judge(doc, only(doc), "# This is fine text."))).toBe("added");
  });

  it("discards an over-edit: ratio on long pieces, count on short ones", () => {
    const long = "The quick brown fox jumps over the lazy dog today.";
    expect(
      reason(judge(long, only(long), "A fast brown cat leaps over the sleepy dog today.")),
    ).toBe("overEdit");
    expect(
      reason(judge(long, only(long), "The quick brown fox jumped over the lazy dog today.")),
    ).toBe("ok");
    const short = "Teh cat sat down.";
    expect(reason(judge(short, only(short), "A dog lay down."))).toBe("ok");
    expect(reason(judge(short, only(short), "A big dog lay up."))).toBe("overEdit");
  });

  it("discards a short piece (<= 3 words) with most words changed into unlike words (a translation in one script)", () => {
    const heading = "# Einleitung";
    expect(reason(judge(heading, only(heading), "Introduction"))).toBe("overEdit");
    const typo = "# Introdution";
    expect(reason(judge(typo, only(typo), "Introduction"))).toBe("ok");
    const three = "Una casa blanca";
    expect(reason(judge(three, only(three), "A white house"))).toBe("overEdit");
    expect(reason(judge(three, only(three), "Una caza blanca"))).toBe("ok");
    const two = "# Teh title";
    expect(reason(judge(two, only(two), "The title"))).toBe("ok");
  });

  it("discards a translation by the shift of script", () => {
    const en = "The weather is very nice today.";
    expect(reason(judge(en, only(en), "今日はとても天気がいいです。"))).toBe("script");
    expect(reason(judge(en, only(en), "Погода сегодня очень хорошая."))).toBe("script");
    const ja = "今日わ天気がいいです。";
    expect(reason(judge(ja, only(ja), "今日は天気がいいです。"))).toBe("ok");
  });

  it("a table cell: a fix lands on the cell's own words; a pipe | line break in the answer discards it", () => {
    const doc = "| Name | Note |\n|:--|--:|\n| Teh quick fox | recieve `x` \\| mail |\n";
    const [, , fox, mail] = pieces(doc);
    if (fox === undefined || mail === undefined) throw new Error("no cells");
    expect(applied(doc, judge(doc, fox, "The quick fox"))).toBe(doc.replace("Teh", "The"));
    expect(mail.text).toBe("recieve ⟦1⟧ ⟦2⟧ mail");
    const at = doc.indexOf("recieve");
    expect(judge(doc, mail, "receive ⟦1⟧ ⟦2⟧ mail")).toMatchObject({
      kind: "ok",
      suggestions: [{ from: at, to: at + 7, original: "recieve", insert: "receive" }],
    });
    expect(reason(judge(doc, fox, "The quick | fox"))).toBe("cell");
    expect(reason(judge(doc, fox, "The quick\nfox"))).toBe("cell");
    // a cell is a label like a list item: no final period added
    expect(judge(doc, fox, "Teh quick fox.")).toMatchObject({ kind: "ok", suggestions: [] });
    expect(applied(doc, judge(doc, fox, "The quick fox."))).toBe(doc.replace("Teh", "The"));
  });

  it("drops a suggestion that only adds a final period to a heading | list item", () => {
    const heading = "# Teh title";
    const v = judge(heading, only(heading), "The title.");
    expect(applied(heading, v)).toBe("# The title");
    const item = "- first item\n- second itme";
    const second = pieces(item)[1];
    if (second === undefined) throw new Error("no piece");
    expect(applied(item, judge(item, second, "second item."))).toBe("- first item\n- second item");
    const ja = "- 日本語の項目";
    expect(judge(ja, only(ja), "日本語の項目。")).toMatchObject({ kind: "ok", suggestions: [] });
    const para = "A paragraph without end";
    expect(applied(para, judge(para, only(para), "A paragraph without end."))).toBe(
      "A paragraph without end.",
    );
  });
});

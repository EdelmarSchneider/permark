import { describe, expect, it } from "vitest";
import { judge, type Verdict } from "./guards";
import { pieces, type Piece } from "./segment";

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

  it("discards a thinking leak and a preamble", () => {
    const doc = "This is fine text.";
    const piece = only(doc);
    expect(reason(judge(doc, piece, "<|channel>thought\nThis is fine text."))).toBe("thinking");
    expect(reason(judge(doc, piece, "Here is the corrected text: This is fine text."))).toBe(
      "preamble",
    );
    expect(reason(judge(doc, piece, "Texto corrigido: This is fine text."))).toBe("preamble");
    const own = "Corrected values are shown below.";
    expect(reason(judge(own, only(own), own))).toBe("ok");
  });

  it("discards tampered markers and changed Markdown symbols", () => {
    const doc = "Use `npm` with *care* now.";
    const piece = only(doc);
    expect(reason(judge(doc, piece, "Use npm with *care* now."))).toBe("markers");
    expect(reason(judge(doc, piece, "Use ⟦1⟧ with care now."))).toBe("symbols");
    expect(reason(judge(doc, piece, "Use ⟦1⟧ with _care_ now."))).toBe("symbols");
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

  it("discards dropped | added text by length ratio", () => {
    const doc = "This sentence is complete and has a clear ending.";
    expect(reason(judge(doc, only(doc), "This sentence is complete."))).toBe("length");
  });

  it("discards a translation by the shift of script", () => {
    const en = "The weather is very nice today.";
    expect(reason(judge(en, only(en), "今日はとても天気がいいです。"))).toBe("script");
    expect(reason(judge(en, only(en), "Погода сегодня очень хорошая."))).toBe("script");
    const ja = "今日わ天気がいいです。";
    expect(reason(judge(ja, only(ja), "今日は天気がいいです。"))).toBe("ok");
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

import { describe, expect, it } from "vitest";
import type { PieceVerdict } from "../local-ai/pieces/review.svelte";
import { pieces, type Piece } from "../local-ai/pieces/segment";
import { around } from "../local-ai/pieces/suggestion";
import { judge, type Pair } from "./guards";

const TO_PT: Pair = { target: "pt-BR", source: null };
const TO_JA: Pair = { target: "ja", source: null };
const TO_EN: Pair = { target: "en-US", source: null };
const TO_KO: Pair = { target: "ko", source: null };
const TO_RU: Pair = { target: "ru", source: null };

function only(doc: string): Piece {
  const [piece] = pieces(doc);
  if (piece === undefined) throw new Error(`no piece in ${doc}`);
  return piece;
}

/** `ok` | `same` | the reason of a discard. */
function reason(v: PieceVerdict): string {
  return v.kind === "discard" ? v.reason : v.kind;
}

/** What the one suggestion of an accepted answer puts in the piece's place. */
function insert(v: PieceVerdict): string {
  if (v.kind !== "ok") throw new Error(`no suggestion: ${reason(v)}`);
  const [s] = v.suggestions;
  if (s === undefined || v.suggestions.length !== 1) throw new Error("not one suggestion");
  return s.insert;
}

/** `doc` with the answer's suggestion applied. */
function applied(doc: string, v: PieceVerdict): string {
  if (v.kind !== "ok") throw new Error(`no suggestion: ${reason(v)}`);
  const [s] = v.suggestions;
  if (s === undefined) throw new Error("no suggestion");
  return doc.slice(0, s.from) + s.insert + doc.slice(s.to);
}

const EN = "The weather is very nice today.";
const PT = "O tempo está muito bom hoje.";
const JA = "今日はとても天気がいいです。";

describe("judge: a translation", () => {
  it("is ONE suggestion for the whole piece, at its exact range", () => {
    const doc = `# Notes\n\n${EN}\n\nBye.\n`;
    const piece = pieces(doc)[1];
    if (piece === undefined) throw new Error("no piece");
    const from = doc.indexOf(EN);
    expect(judge(doc, piece, PT, TO_PT)).toEqual({
      kind: "ok",
      suggestions: [
        {
          from,
          to: from + EN.length,
          original: EN,
          insert: PT,
          ...around(doc, from, from + EN.length),
        },
      ],
    });
    expect(applied(doc, judge(doc, piece, ` ${PT}\n`, TO_PT))).toBe(`# Notes\n\n${PT}\n\nBye.\n`);
  });

  it("carries the text around the piece (to show it, to find it again)", () => {
    const doc = "- first item of the list\n- second item of the list\n- third item\n";
    const second = pieces(doc)[1];
    if (second === undefined) throw new Error("no piece");
    const v = judge(doc, second, "segundo item da lista", TO_PT);
    expect(v).toMatchObject({
      kind: "ok",
      suggestions: [{ before: "- first item of the list\n- ", after: "\n- third item\n" }],
    });
  });

  it("may move the markers: each one once, anywhere", () => {
    const doc = "Run `npm install` and then open [the guide](docs/guide.md) now.";
    const piece = only(doc);
    expect(piece.text).toBe("Run ⟦1⟧ and then open [the guide]⟦2⟧ now.");
    const v = judge(doc, piece, "Abra [o guia]⟦2⟧ agora, depois de executar ⟦1⟧.", TO_PT);
    expect(applied(doc, v)).toBe(
      "Abra [o guia](docs/guide.md) agora, depois de executar `npm install`.",
    );
  });

  it("passes on what the structure check discards", () => {
    const doc = "Run `npm install` and then open [the guide](docs/guide.md) now.";
    const piece = only(doc);
    const check = (output: string): string => reason(judge(doc, piece, output, TO_PT));
    expect(check("Abra [o guia]⟦2⟧ agora.")).toBe("markers");
    expect(check("Abra [o guia]⟦2⟧ agora, depois de ⟦1⟧ e ⟦1⟧.")).toBe("markers");
    expect(check("Abra o guia⟦2⟧ agora, depois de executar ⟦1⟧.")).toBe("symbols");
    expect(check("<|channel>thought\nAbra [o guia]⟦2⟧ agora, depois de ⟦1⟧.")).toBe("thinking");
    expect(check("Here is the translation: Abra [o guia]⟦2⟧ agora, depois de ⟦1⟧.")).toBe(
      "preamble",
    );
    expect(check("Sure, here it is: Abra [o guia]⟦2⟧ agora, depois de ⟦1⟧.")).toBe("preamble");
  });

  it("an answer with Markdown the piece does not have is no translation of it", () => {
    const doc = "Run `npm install` and then open [the guide](docs/guide.md) now.";
    const piece = only(doc);
    const check = (output: string): string => reason(judge(doc, piece, output, TO_PT));
    expect(check("Abra [o guia](javascript:alert(1))⟦2⟧ agora, depois de executar ⟦1⟧.")).toBe(
      "added",
    );
    expect(check("Abra ![o guia]⟦2⟧ agora, depois de executar ⟦1⟧.")).toBe("added");
    expect(check("Abra [o guia]⟦2⟧ agora <img src=https://x.y/z>, depois de ⟦1⟧.")).toBe("added");
    expect(check("Abra [o guia]⟦2⟧ agora, depois de executar ⟦1⟧ e `rm -rf`.")).toBe("added");
    // the first paragraph of a document turned into its front matter, a paragraph into a heading
    expect(reason(judge(EN, only(EN), "---\ntitle: O tempo está bom\n---", TO_PT))).toBe("added");
    expect(reason(judge(EN, only(EN), `# ${PT}`, TO_PT))).toBe("added");
    // what the piece itself has stays
    const math = "When a < b the list is sorted -> nothing is done.";
    expect(
      insert(judge(math, only(math), "Quando a < b a lista está em ordem -> nada é feito.", TO_PT)),
    ).toBe("Quando a < b a lista está em ordem -> nada é feito.");
  });

  it("a piece that itself opens like an answer's opening is not held to it, in any language", () => {
    const doc = "Translation: done by hand.";
    expect(insert(judge(doc, only(doc), "Tradução: feita à mão.", TO_PT))).toBe(
      "Tradução: feita à mão.",
    );
    const pt = "Tradução: feita à mão.";
    expect(reason(judge(pt, only(pt), "Translation: done by hand.", TO_EN))).toBe("ok");
  });

  it("an answer without a letter is no translation", () => {
    expect(reason(judge("Hello.", only("Hello."), "", TO_PT))).toBe("empty");
    expect(reason(judge("Hello.", only("Hello."), " … ", TO_PT))).toBe("empty");
    expect(reason(judge("Hello.", only("Hello."), '""', { target: "qaa", source: null }))).toBe(
      "empty",
    );
  });
});

describe("judge: the same text as the piece", () => {
  it("is nothing to suggest, whatever the target's script", () => {
    expect(judge(EN, only(EN), EN, TO_PT)).toEqual({ kind: "same" });
    expect(judge("# PerMark CLI", only("# PerMark CLI"), "PerMark CLI", TO_JA)).toEqual({
      kind: "same",
    });
  });

  it("is told after NFC and with any whitespace as one space", () => {
    const doc = "The café is  open\ntoday.";
    expect(judge(doc, only(doc), " The cafe\u0301 is open today. ", TO_PT)).toEqual({
      kind: "same",
    });
  });

  it("with its markers where they were", () => {
    const doc = "Run `npm install` first.";
    expect(judge(doc, only(doc), "Run ⟦1⟧ first.", TO_PT)).toEqual({ kind: "same" });
  });
});

describe("judge: quotation marks around the answer", () => {
  it("are taken off when the piece has none", () => {
    for (const quoted of [`"${PT}"`, `“${PT}”`, `«${PT}»`, `„${PT}“`, `'${PT}'`, `「${PT}」`])
      expect(insert(judge(EN, only(EN), quoted, TO_PT)), quoted).toBe(PT);
    // the piece itself in quotation marks: nothing to suggest
    expect(judge(EN, only(EN), `"${EN}"`, TO_PT)).toEqual({ kind: "same" });
  });

  it("stay when the piece has them, or when they are not one pair around the whole answer", () => {
    const quoted = `"${EN}"`;
    expect(insert(judge(quoted, only(quoted), `«${PT}»`, TO_PT))).toBe(`«${PT}»`);
    const said = 'He said "hello" and she said "goodbye" to him.';
    const answer = '"Olá", disse ele, e ela disse "adeus"';
    expect(insert(judge(said, only(said), answer, TO_PT))).toBe(answer);
  });
});

describe("judge: an answer that is more than the translation", () => {
  it("an empty line in it (a note, a second answer) discards it", () => {
    expect(reason(judge(EN, only(EN), `${PT}\n\nNote: translated literally.`, TO_PT))).toBe(
      "blankLine",
    );
    expect(reason(judge(EN, only(EN), `${PT}\n \t\nOu: ${PT}`, TO_PT))).toBe("blankLine");
    expect(reason(judge(EN, only(EN), `${PT}\n\n`, TO_PT))).toBe("ok");
  });
});

describe("judge: the target's script", () => {
  it("an answer mostly in another script is discarded", () => {
    // the piece again in other words: the words it shares with the piece are left out, the rest tell the script
    expect(reason(judge(EN, only(EN), "The weather is really quite good today.", TO_JA))).toBe(
      "script",
    );
    expect(reason(judge(EN, only(EN), PT, TO_JA))).toBe("script");
    expect(reason(judge(EN, only(EN), JA, TO_PT))).toBe("script");
    expect(reason(judge(EN, only(EN), "Погода сегодня очень хорошая.", TO_PT))).toBe("script");
  });

  it("an answer in the target's script passes: Japanese = Han + Kana, Korean = Hangul + Han, Chinese = Han", () => {
    expect(insert(judge(EN, only(EN), JA, TO_JA))).toBe(JA);
    expect(
      reason(judge(EN, only(EN), "오늘은 날씨가 아주 좋습니다.", { target: "ko", source: null })),
    ).toBe("ok");
    expect(
      reason(judge(EN, only(EN), "今天天气非常好。", { target: "zh-Hans", source: null })),
    ).toBe("ok");
    expect(
      reason(judge(EN, only(EN), "今天天氣非常好。", { target: "zh-Hant", source: null })),
    ).toBe("ok");
    expect(
      reason(judge(EN, only(EN), "Погода сегодня очень хорошая.", { target: "ru", source: null })),
    ).toBe("ok");
    expect(
      reason(judge(EN, only(EN), "الطقس جميل جدا اليوم.", { target: "ar", source: null })),
    ).toBe("ok");
    // names kept in Latin letters beside the translation
    expect(reason(judge(EN, only(EN), "今日は PerMark の天気がとてもいいです。", TO_JA))).toBe(
      "ok",
    );
  });

  it("words the answer copied from the piece (a product name, a file name) do not count against its script", () => {
    // 6 Latin letters of 9: the name alone would outweigh the translation around it
    const heading = "# Node.js guide";
    expect(insert(judge(heading, only(heading), "Node.jsガイド", TO_JA))).toBe("Node.jsガイド");
    const app = "PerMark is a Markdown app";
    expect(insert(judge(app, only(app), "PerMark は Markdown アプリです", TO_JA))).toBe(
      "PerMark は Markdown アプリです",
    );
    // the period that ends the sentence is no part of the file name; names with `_` and `-` inside
    const file = "Open settings.local.json.";
    expect(
      reason(
        judge(file, only(file), "打开settings.local.json。", { target: "zh-Hans", source: null }),
      ),
    ).toBe("ok");
    const flag = "Set max_retries or pass --no-cache.";
    expect(
      reason(judge(flag, only(flag), "max_retries를 설정하거나 --no-cache를 전달하세요.", TO_KO)),
    ).toBe("ok");
    // the same in a script written with spaces between the words
    expect(reason(judge(app, only(app), "PerMark — это приложение Markdown", TO_RU))).toBe("ok");
    // a name the piece itself writes right beside its own words (Korean puts the particle on it)
    const ko = "PerMark에서 Markdown 파일을 엽니다.";
    expect(reason(judge(ko, only(ko), "PerMarkでMarkdownファイルを開きます。", TO_JA))).toBe("ok");
  });

  it("a word in the target's script counts for it, copied or not (Chinese and Japanese share words)", () => {
    const zh = "設定 完成";
    expect(reason(judge(zh, only(zh), "設定 OK", TO_JA))).toBe("ok");
    expect(reason(judge(zh, only(zh), "Setup OK", TO_JA))).toBe("script");
  });

  it("only a word written exactly as in the piece is a copy", () => {
    const heading = "# Node.js guide";
    const check = (output: string): string => reason(judge(heading, only(heading), output, TO_JA));
    // another word in Latin letters, the name in another letter case, the name inside a longer word
    expect(check("Node.js handbook 入門")).toBe("script");
    expect(check("node.js ガイド")).toBe("script");
    expect(check("Node.jsx ガイド")).toBe("script");
    // an answer of copied words alone has no letter left to hold to a script (Japanese has no article)
    const api = "# The Node.js API";
    expect(insert(judge(api, only(api), "Node.js API", TO_JA))).toBe("Node.js API");
  });

  it("the piece left as it is says `same` before the script is asked; left in its language in other words, `script`", () => {
    expect(judge(EN, only(EN), EN, TO_JA)).toEqual({ kind: "same" });
    expect(reason(judge(EN, only(EN), "Today the weather is very pleasant.", TO_JA))).toBe(
      "script",
    );
  });

  it("a target written in Latin letters is held as before", () => {
    const ja = "この機能は PerMark の設定ページで変更できます。";
    const en = "You can change this feature on the settings page of PerMark.";
    expect(insert(judge(ja, only(ja), en, TO_EN))).toBe(en);
    // left in Japanese, in other words: the name it shares with the piece does not make it English
    expect(reason(judge(ja, only(ja), "PerMark の設定ページでこの機能を変更します。", TO_EN))).toBe(
      "script",
    );
    expect(reason(judge(EN, only(EN), PT, TO_PT))).toBe("ok");
    expect(reason(judge(EN, only(EN), JA, TO_PT))).toBe("script");
  });

  it("a target whose script is not known is not held to one", () => {
    expect(reason(judge(EN, only(EN), PT, { target: "qaa", source: null }))).toBe("ok");
    expect(reason(judge(EN, only(EN), JA, { target: "not a tag", source: null }))).toBe("ok");
  });
});

describe("judge: the answer's length against the piece's (pieces of 20 chars and more)", () => {
  const JA_LONG = "今日はとても天気がいいので、公園へ散歩に行きます。";

  it("the same kind of script: between half and twice", () => {
    expect(reason(judge(EN, only(EN), "Tempo bom.", TO_PT))).toBe("length");
    expect(reason(judge(EN, only(EN), "O tempo está bom.", TO_PT))).toBe("ok");
    expect(reason(judge(EN, only(EN), `${PT} ${PT} E amanhã também.`, TO_PT))).toBe("length");
  });

  it("alphabetic text into Han | Kana | Hangul: much shorter is fine, longer is not", () => {
    expect(JA.length / EN.length).toBeLessThan(0.5);
    expect(reason(judge(EN, only(EN), JA, TO_JA))).toBe("ok");
    expect(reason(judge(EN, only(EN), "天気です。", TO_JA))).toBe("length");
    expect(reason(judge(EN, only(EN), `${JA}${JA}${JA}`, TO_JA))).toBe("length");
  });

  it("Han | Kana | Hangul into alphabetic text: at least as long, up to five times", () => {
    expect(JA_LONG.length).toBeGreaterThanOrEqual(20);
    const en = "The weather is very nice today, so I am going for a walk in the park.";
    expect(en.length / JA_LONG.length).toBeGreaterThan(2);
    expect(reason(judge(JA_LONG, only(JA_LONG), en, TO_EN))).toBe("ok");
    expect(reason(judge(JA_LONG, only(JA_LONG), "Nice day, a walk.", TO_EN))).toBe("length");
    expect(reason(judge(JA_LONG, only(JA_LONG), `${en} ${en}`, TO_EN))).toBe("length");
  });

  it("a short piece is not held to a ratio", () => {
    expect(reason(judge("Hi.", only("Hi."), "こんにちは、みなさん。", TO_JA))).toBe("ok");
  });
});

describe("judge: line breaks", () => {
  it("a piece written on one line gets its answer on one line", () => {
    expect(insert(judge(EN, only(EN), "O tempo está\nmuito bom hoje.", TO_PT))).toBe(PT);
    expect(insert(judge(EN, only(EN), "O tempo está  \n  muito bom hoje.", TO_PT))).toBe(PT);
    const heading = "# Getting started with the app";
    expect(
      applied(heading, judge(heading, only(heading), "Primeiros\npassos com o app", TO_PT)),
    ).toBe("# Primeiros passos com o app");
  });

  it("joined without a space between two chars of a script written without spaces", () => {
    expect(insert(judge(EN, only(EN), "今日はとても\n天気がいいです。", TO_JA))).toBe(JA);
    expect(insert(judge(EN, only(EN), "今日は、\nとても天気がいいです。", TO_JA))).toBe(
      "今日は、とても天気がいいです。",
    );
    // Korean separates words: the break was one
    expect(
      insert(judge(EN, only(EN), "오늘은 날씨가\n아주 좋습니다.", { target: "ko", source: null })),
    ).toBe("오늘은 날씨가 아주 좋습니다.");
  });

  it("a piece of several lines keeps the answer's own lines", () => {
    const doc = "The weather is very nice today\nand tomorrow it will rain.";
    const answer = "O tempo está muito bom hoje\ne amanhã vai chover.";
    expect(insert(judge(doc, only(doc), answer, TO_PT))).toBe(answer);
  });

  it("a table cell: a pipe | line break in the answer discards it", () => {
    const doc = "| Name | Note |\n|---|---|\n| The quick fox | 12 |\n";
    const fox = pieces(doc)[2];
    if (fox === undefined) throw new Error("no cell");
    expect(applied(doc, judge(doc, fox, "A raposa rápida", TO_PT))).toBe(
      doc.replace("The quick fox", "A raposa rápida"),
    );
    expect(reason(judge(doc, fox, "A raposa | rápida", TO_PT))).toBe("cell");
    expect(reason(judge(doc, fox, "A raposa\nrápida", TO_PT))).toBe("cell");
  });
});

describe("judge: markers that hold the lines of a block", () => {
  const QUOTE = "> The weather is nice\n> and the sun shines.\n";

  it("a quote's `>` stays where it opens a line after the first", () => {
    const piece = only(QUOTE);
    expect(piece.text).toBe("The weather is nice\n⟦1⟧ and the sun shines.");
    expect(
      applied(QUOTE, judge(QUOTE, piece, "O tempo está bom\n⟦1⟧ e o sol brilha.", TO_PT)),
    ).toBe("> O tempo está bom\n> e o sol brilha.\n");
    const check = (output: string): string => reason(judge(QUOTE, piece, output, TO_PT));
    // into a line
    expect(check("O tempo ⟦1⟧ está bom\ne o sol brilha.")).toBe("lines");
    // its line break gone
    expect(check("O tempo está bom ⟦1⟧ e o sol brilha.")).toBe("lines");
    // onto the first line, which has the block's own `>` before it
    expect(check("⟦1⟧ O tempo está bom\ne o sol brilha.")).toBe("lines");
  });

  it("the marks of a quote in a quote lead their line together", () => {
    const doc = "> > The weather is nice\n> > and the sun shines.\n";
    const piece = only(doc);
    expect(piece.text).toBe("The weather is nice\n⟦1⟧ ⟦2⟧ and the sun shines.");
    expect(
      applied(doc, judge(doc, piece, "O tempo está bom\n⟦1⟧ ⟦2⟧ e o sol brilha.", TO_PT)),
    ).toBe("> > O tempo está bom\n> > e o sol brilha.\n");
    expect(reason(judge(doc, piece, "O tempo está bom\n⟦1⟧ e o sol ⟦2⟧ brilha.", TO_PT))).toBe(
      "lines",
    );
  });

  it("a hard break may move with the words, never to an end of the answer or beside a line break", () => {
    const doc = "First line of the text  \nsecond line of the text.";
    const piece = only(doc);
    expect(piece.text).toBe("First line of the text⟦1⟧second line of the text.");
    expect(
      insert(judge(doc, piece, "Primeira linha⟦1⟧do texto, segunda linha do texto.", TO_PT)),
    ).toBe("Primeira linha  \ndo texto, segunda linha do texto.");
    const check = (output: string): string => reason(judge(doc, piece, output, TO_PT));
    expect(check("Primeira linha do texto, segunda linha do texto.⟦1⟧")).toBe("lines");
    expect(check("⟦1⟧Primeira linha do texto, segunda linha do texto.")).toBe("lines");
    expect(check("Primeira linha do texto⟦1⟧\nsegunda linha do texto.")).toBe("blankLine");
  });

  it("a hard break written with a backslash is held the same way", () => {
    const doc = "First line of the text\\\nsecond line of the text.";
    const piece = only(doc);
    expect(piece.markers).toEqual(["\\\n"]);
    const check = (output: string): string => reason(judge(doc, piece, output, TO_PT));
    expect(check("Primeira linha do texto,⟦1⟧segunda linha do texto.")).toBe("ok");
    expect(check("Primeira linha do texto, segunda linha do texto.⟦1⟧")).toBe("lines");
    expect(check("⟦1⟧Primeira linha do texto, segunda linha do texto.")).toBe("lines");
  });

  it("a hard break in a quote carries the next line's `>` with it", () => {
    const doc = "> First line of the text  \n> second line of the text.\n";
    const piece = only(doc);
    expect(piece.text).toBe("First line of the text⟦1⟧ second line of the text.");
    expect(
      applied(doc, judge(doc, piece, "Primeira linha⟦1⟧ do texto, segunda linha.", TO_PT)),
    ).toBe("> Primeira linha  \n> do texto, segunda linha.\n");
    // at the end it would leave a quote line without text
    expect(reason(judge(doc, piece, "Primeira linha do texto, segunda linha.⟦1⟧", TO_PT))).toBe(
      "lines",
    );
  });

  it("a quote's `>` alone on a line would end the paragraph there", () => {
    const doc = "> > The weather is nice\n> > and the sun shines.\n";
    const piece = only(doc);
    expect(reason(judge(doc, piece, "O tempo está bom\n⟦1⟧\n⟦2⟧ e o sol brilha.", TO_PT))).toBe(
      "blankLine",
    );
  });
});

import { describe, expect, it } from "vitest";
import { language } from "../../core/i18n";
import { effective, infoOf, MAX_TOKENIZERS, TOKENIZERS } from "./tokenizers";

const ids = (list: readonly { id: string }[]): string[] => list.map((info) => info.id);

describe("the tokenizers", () => {
  it("lists the two exact encodings and the three estimates, each marked as what it is", () => {
    expect(TOKENIZERS.map((t) => [t.id, t.exact])).toEqual([
      ["o200k", true],
      ["cl100k", true],
      ["claude", false],
      ["gemini", false],
      ["chars4", false],
    ]);
    expect(infoOf("claude")?.short).toBe("Claude");
    expect(infoOf("nope")).toBeNull();
  });

  it("names the rule of thumb in the interface language (product names stay)", async () => {
    try {
      expect(infoOf("chars4")?.short).toBe("chars ÷ 4");
      await language.choose("pt-BR", []);
      expect(infoOf("chars4")?.short).toBe("caracteres ÷ 4");
      expect(infoOf("gemini")?.short).toBe("Gemini");
    } finally {
      await language.choose("en", []);
    }
  });

  it("the project's choice wins when it names a known tokenizer", () => {
    expect(ids(effective(["o200k"], ["claude"]))).toEqual(["claude"]);
    expect(ids(effective(["o200k"], []))).toEqual(["o200k"]);
    expect(ids(effective(["o200k"], ["nope"]))).toEqual(["o200k"]);
    expect(ids(effective(["o200k"], ["nope", "gemini"]))).toEqual(["gemini"]);
  });

  it("drops unknown ids and repeats and keeps three", () => {
    expect(ids(effective(["x", "claude", "claude", "o200k", "gemini", "chars4"], []))).toEqual([
      "claude",
      "o200k",
      "gemini",
    ]);
    expect(MAX_TOKENIZERS).toBe(3);
    expect(effective([], [])).toEqual([]);
  });
});

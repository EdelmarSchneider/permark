import { Schema } from "@milkdown/kit/prose/model";
import { describe, expect, it } from "vitest";
import { matchesIn } from "./find";

const schema = new Schema({
  nodes: { doc: { content: "paragraph+" }, paragraph: { content: "text*" }, text: {} },
});

const docOf = (...lines: string[]) =>
  schema.node(
    "doc",
    null,
    lines.map((line) => schema.node("paragraph", null, line === "" ? [] : [schema.text(line)])),
  );

const found = (doc: ReturnType<typeof docOf>, query: string): string[] =>
  matchesIn(doc, query).matches.map((m) => doc.textBetween(m.from, m.to));

describe("matchesIn", () => {
  it("stays case-insensitive in a text with a dotted İ, and the dotted and dotless i match each other", () => {
    // "İ".toLowerCase() is two characters: positions after it would shift
    const doc = docOf("İSTANBUL, İstanbul ve Işık", "IŞIK and ışık");
    expect(found(doc, "istanbul")).toEqual(["İSTANBUL", "İstanbul"]);
    expect(found(doc, "İstanbul")).toEqual(["İSTANBUL", "İstanbul"]);
    expect(found(doc, "ve")).toEqual(["ve"]);
    expect(found(doc, "ışık")).toEqual(["Işık", "IŞIK", "ışık"]);
  });
});

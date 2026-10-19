// @vitest-environment jsdom
import { describe, expect, it } from "vitest";
import { parseXml } from "./xml";

describe("parseXml", () => {
  it("shows elements with attributes, text on its row, children on demand", () => {
    const parsed = parseXml(
      '<?xml version="1.0"?><root id="1"><!-- note --><item name="a">one</item><item name="b"><sub/></item><empty/></root>',
    );
    expect(parsed.ok).toBe(true);
    if (!parsed.ok) return;
    expect(parsed.root.label).toBe("root");
    expect(parsed.root.summary).toBe('id="1"');
    const kids = parsed.root.children?.() ?? [];
    expect(kids.map((k) => `${k.kind}:${k.label ?? ""}:${k.summary}`)).toEqual([
      "comment::<!-- note -->",
      'element:item:name="a"  one',
      'element:item:name="b"',
      "element:empty:",
    ]);
    expect(kids[1]?.children).toBeNull();
    expect(kids[2]?.children?.().map((k) => k.label)).toEqual(["sub"]);
    expect(kids[3]?.children).toBeNull();
  });

  it("keeps mixed content in order and drops blank text", () => {
    const parsed = parseXml("<p>Hello <b>bold</b> world\n  </p>");
    expect(parsed.ok && (parsed.root.children?.() ?? []).map((k) => k.summary)).toEqual([
      "Hello",
      "bold",
      "world",
    ]);
  });

  it("reads CDATA as text", () => {
    const parsed = parseXml("<a><![CDATA[1 < 2]]></a>");
    expect(parsed.ok && parsed.root.summary).toBe("1 < 2");
  });

  it("refuses what is not well-formed", () => {
    for (const bad of ["", "<a>", "<a></b>", "text", "<a><b></a></b>", "<a b=1/>"]) {
      expect(parseXml(bad).ok, bad).toBe(false);
    }
  });

  it("clips very long rows", () => {
    const parsed = parseXml(`<a>${"x".repeat(5000)}</a>`);
    expect(parsed.ok && parsed.root.summary.length).toBeLessThan(400);
  });
});

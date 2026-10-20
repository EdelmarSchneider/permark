import { describe, expect, it } from "vitest";
import { formatQuote, formatReference, quoteLines } from "./reference";

const ref = { path: "docs/specs/live-feed.md", section: "event-model", line: 42 };

describe("formatReference", () => {
  it("fills the default template", () => {
    expect(formatReference("", ref)).toBe("docs/specs/live-feed.md#event-model");
    expect(formatReference("   ", ref)).toBe("docs/specs/live-feed.md#event-model");
  });

  it("fills every placeholder, repeats included", () => {
    expect(formatReference("@{path}:{line} ({name}) {path}", ref)).toBe(
      "@docs/specs/live-feed.md:42 (live-feed.md) docs/specs/live-feed.md",
    );
  });

  it("takes the joiner with a missing value", () => {
    const whole = { path: "a.md", section: null, line: null };
    expect(formatReference("{path}#{section}", whole)).toBe("a.md");
    expect(formatReference("{path}:{line}", whole)).toBe("a.md");
    expect(formatReference("{path} ({section})", whole)).toBe("a.md ()");
    expect(formatReference("{path}#{section}", { ...whole, section: "" })).toBe("a.md");
  });

  it("leaves unknown placeholders and braces alone", () => {
    expect(formatReference("{path} {unknown} {{path}} {", ref)).toBe(
      "docs/specs/live-feed.md {unknown} {docs/specs/live-feed.md} {",
    );
  });

  it("does not let a value inject placeholders", () => {
    const tricky = { path: "a{line}.md", section: null, line: 7 };
    expect(formatReference("{path}:{line}", tricky)).toBe("a{line}.md:7");
  });
});

describe("quotes", () => {
  it("quotes lines, keeps blank ones as empty quote lines, trims, normalizes line breaks", () => {
    expect(quoteLines("one\r\n\r\ntwo  \n")).toBe("> one\n>\n> two");
    expect(quoteLines("  \n x \n")).toBe("> x");
    expect(quoteLines("")).toBe(">");
  });

  it("puts the reference above the quote", () => {
    expect(formatQuote("{path}#{section}", ref, "a\nb")).toBe(
      "docs/specs/live-feed.md#event-model\n> a\n> b",
    );
  });
});

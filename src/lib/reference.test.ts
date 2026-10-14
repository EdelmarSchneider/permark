import { describe, expect, it } from "vitest";
import { DEFAULT_REF_TEMPLATE, formatQuote, formatReference, quoteLines } from "./reference";

const label = (name: string): string => `section "${name}"`;

describe("formatReference", () => {
  it("uses the default template", () => {
    expect(formatReference(DEFAULT_REF_TEMPLATE, "docs/specs/live-feed.md")).toBe(
      "@docs/specs/live-feed.md",
    );
  });
  it("supports custom templates and repeated placeholders", () => {
    expect(formatReference("see {path} ({path})", "a.md")).toBe("see a.md (a.md)");
    expect(formatReference("file://{path}", "a b.md")).toBe("file://a b.md");
  });
  it("falls back to the default for an empty template", () => {
    expect(formatReference("  ", "a.md")).toBe("@a.md");
  });
});

describe("quoteLines", () => {
  it("prefixes every line and keeps blank lines as empty quotes", () => {
    expect(quoteLines("one\n\ntwo")).toBe("> one\n>\n> two");
  });
  it("normalizes CRLF and trims the ends", () => {
    expect(quoteLines("\r\n one  \r\ntwo \r\n")).toBe("> one\n> two");
  });
});

describe("formatQuote", () => {
  it("builds the reference, the section and the quote", () => {
    const text = formatQuote({
      template: "@{path}",
      path: "docs/specs/live-feed.md",
      section: "Event model",
      text: "emitting it, so a save that was undone never reaches the feed.",
      sectionLabel: label,
    });
    expect(text).toBe(
      '@docs/specs/live-feed.md (section "Event model")\n> emitting it, so a save that was undone never reaches the feed.',
    );
  });
  it("omits the section when the selection is above the first heading", () => {
    const text = formatQuote({
      template: "@{path}",
      path: "a.md",
      section: null,
      text: "x",
      sectionLabel: label,
    });
    expect(text).toBe("@a.md\n> x");
  });
});

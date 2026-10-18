// @vitest-environment jsdom
import { describe, expect, it } from "vitest";
import { plantumlFetches, sanitizePlantumlSvg } from "./plantuml";

describe("plantumlFetches", () => {
  it("allows a diagram that only mentions a web address", () => {
    expect(plantumlFetches("@startuml\nAlice -> Bob: see https://example.com\n@enduml")).toBe(
      false,
    );
  });

  it("refuses an include, including one from the standard library", () => {
    expect(plantumlFetches('!include <C4/C4_Context.puml>\nPerson(p, "P")')).toBe(true);
    expect(plantumlFetches("!include https://example.com/lib.puml\nAlice -> Bob")).toBe(true);
    expect(plantumlFetches("!includeurl http://example.com/a.puml")).toBe(true);
    expect(plantumlFetches("!include_once https://cdn.example/a.puml")).toBe(true);
    expect(plantumlFetches("!import https://example.com/data.json")).toBe(true);
    expect(plantumlFetches("!theme plain from https://example.com/themes")).toBe(true);
  });
});

describe("sanitizePlantumlSvg", () => {
  it("keeps a plain drawing", () => {
    const svg = '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 40 20"><text>Hi</text></svg>';
    expect(sanitizePlantumlSvg(svg)).toContain("<text>Hi</text>");
  });

  it("drops a script and a javascript link", () => {
    const svg =
      '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 10 10"><script>alert(1)</script><a href="javascript:alert(1)">x</a></svg>';
    const clean = sanitizePlantumlSvg(svg);
    expect(clean).not.toContain("script");
    expect(clean).not.toContain("javascript:");
  });

  it("drops a remote image and keeps an ordinary link", () => {
    const svg =
      '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 10 10"><image href="https://evil.example/a.png"/><a href="https://example.com/docs">docs</a></svg>';
    const clean = sanitizePlantumlSvg(svg) ?? "";
    expect(clean).not.toContain("evil.example");
    expect(clean).toContain("https://example.com/docs");
  });

  it("accepts the engine's svg when xmlns is written twice", () => {
    const svg =
      '<svg xmlns="http://www.w3.org/2000/svg" xmlns="http://www.w3.org/2000/svg" viewBox="0 0 10 10"><text>Hi</text></svg>';
    expect(sanitizePlantumlSvg(svg)).toContain("<text>Hi</text>");
  });

  it("rejects markup that is not an svg", () => {
    expect(sanitizePlantumlSvg("<p>no</p>")).toBeNull();
    expect(sanitizePlantumlSvg("not xml")).toBeNull();
  });
});

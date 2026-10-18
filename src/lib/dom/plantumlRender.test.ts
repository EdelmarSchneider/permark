// @vitest-environment jsdom
import { beforeAll, describe, expect, it } from "vitest";
import { renderPlantuml } from "./plantuml";

beforeAll(() => {
  // The engine measures text through canvas and SVG. jsdom has neither, so the stubs stand in for the webview.
  HTMLCanvasElement.prototype.getContext = (() =>
    new Proxy(
      { font: "12px sans-serif" },
      {
        get(target, prop) {
          if (prop in target) return target[prop as keyof typeof target];
          return () => ({ width: 8 });
        },
      },
    )) as unknown as typeof HTMLCanvasElement.prototype.getContext;
  (SVGElement.prototype as unknown as { getBBox: () => DOMRect }).getBBox = () =>
    ({
      x: 0,
      y: 0,
      width: 48,
      height: 16,
      top: 0,
      left: 0,
      right: 48,
      bottom: 16,
      toJSON: () => ({}),
    }) as DOMRect;
});

describe("renderPlantuml", () => {
  it("draws a sequence diagram without leaving the machine", async () => {
    const svg = await renderPlantuml("@startuml\nAlice -> Bob: Hello\n@enduml", "light");
    expect(svg).toContain("<svg");
    expect(svg).toContain("Alice");
    expect(svg).toContain("Bob");
  }, 30_000);

  it("draws a class diagram", async () => {
    const svg = await renderPlantuml(
      "@startuml\nclass Foo\nclass Bar\nFoo --> Bar\n@enduml",
      "dark",
    );
    expect(svg).toContain("<svg");
    expect(svg).toContain("Foo");
    expect(svg).toContain("Bar");
    expect(
      (globalThis as { __plantumlSmetanaFallbackNoted?: boolean }).__plantumlSmetanaFallbackNoted,
    ).toBeUndefined();
  }, 30_000);

  it("refuses a remote include before drawing", async () => {
    const svg = await renderPlantuml("!include https://example.com/x.puml\nAlice -> Bob", "light");
    expect(svg).toBeNull();
  });
});

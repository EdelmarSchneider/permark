// @vitest-environment jsdom
import { beforeEach, describe, expect, it } from "vitest";
import { diagrams, sanitizeSvg, viewBoxWidth } from "./diagram";

const svg = (inner: string): string =>
  `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 100 50">${inner}</svg>`;

describe("sanitizeSvg", () => {
  it("keeps a plain drawing", () => {
    const clean = sanitizeSvg(
      svg('<rect width="10" height="10" fill="red"/><text x="1" y="2">hi</text>'),
    );
    expect(clean).toContain("<rect");
    expect(clean).toContain("hi");
  });

  it("drops scripts, foreign content and event handlers", () => {
    const clean = sanitizeSvg(
      svg(
        '<script>alert(1)</script><foreignObject><div>x</div></foreignObject><rect onclick="x()" onload="y()" width="1" height="1"/><iframe src="x"/>',
      ),
    );
    expect(clean).not.toBeNull();
    for (const bad of ["script", "foreignObject", "onclick", "onload", "iframe", "alert"]) {
      expect(clean).not.toContain(bad);
    }
    expect(clean).toContain("<rect");
  });

  it("removes remote and script references but keeps in-page ones", () => {
    const clean = sanitizeSvg(
      svg(
        '<image href="https://example.com/x.png"/><image xlink:href="data:image/png;base64,AAAA" xmlns:xlink="http://www.w3.org/1999/xlink"/><a href="javascript:alert(1)"><text>l</text></a><use href="#shape"/><image href="//evil/x"/>',
      ),
    );
    expect(clean).not.toContain("example.com");
    expect(clean).not.toContain("javascript");
    expect(clean).not.toContain("evil");
    expect(clean).not.toContain("data:image");
    expect(clean).toContain('href="#shape"');
    expect(clean).toContain("<text>l</text>");
  });

  it("drops animations that could set a link and script addresses written with spaces", () => {
    const clean = sanitizeSvg(
      svg(
        '<a><set attributeName="href" to="javascript:alert(1)"/><animate attributeName="href" values="javascript:x"/><text>s</text></a><a href="java\tscript:alert(2)"><text>t</text></a><a href=" javascript:alert(3)"><text>u</text></a>',
      ),
    );
    expect(clean).not.toContain("<set");
    expect(clean).not.toContain("<animate");
    expect(clean).not.toContain("script:");
    expect(clean).toContain("<text>t</text>");
  });

  it("refuses what is not SVG or not well-formed", () => {
    expect(sanitizeSvg("<html><body>x</body></html>")).toBeNull();
    expect(sanitizeSvg("not xml")).toBeNull();
    expect(sanitizeSvg("<svg><g></svg>")).toBeNull();
    expect(sanitizeSvg("")).toBeNull();
  });

  it("accepts the doubled xmlns some engines write", () => {
    const doubled =
      '<svg xmlns="http://www.w3.org/2000/svg" xmlns="http://www.w3.org/2000/svg"><g/></svg>';
    expect(sanitizeSvg(doubled)).not.toBeNull();
  });
});

describe("viewBoxWidth", () => {
  it("reads the width of a viewBox", () => {
    expect(viewBoxWidth("0 0 320.5 100")).toBe(320.5);
    expect(viewBoxWidth("0,0,50,20")).toBe(50);
    expect(viewBoxWidth("0 0 0 10")).toBeNull();
    expect(viewBoxWidth("0 0 x 10")).toBeNull();
    expect(viewBoxWidth("1 2 3")).toBeNull();
    expect(viewBoxWidth(null)).toBeNull();
  });
});

describe("diagrams queue", () => {
  beforeEach(() => diagrams.clear());

  it("draws one at a time, in order", async () => {
    const log: string[] = [];
    const job = (name: string, ms: number) => async () => {
      log.push(`start ${name}`);
      await new Promise((r) => setTimeout(r, ms));
      log.push(`end ${name}`);
      return name;
    };
    const [a, b] = await Promise.all([
      diagrams.draw("a", job("a", 20)),
      diagrams.draw("b", job("b", 1)),
    ]);
    expect([a, b]).toEqual(["a", "b"]);
    expect(log).toEqual(["start a", "end a", "start b", "end b"]);
  });

  it("caches results and does not cache failures", async () => {
    let runs = 0;
    const ok = () => {
      runs += 1;
      return Promise.resolve("<svg/>");
    };
    await diagrams.draw("k", ok);
    await diagrams.draw("k", ok);
    expect(runs).toBe(1);
    let failures = 0;
    const failing = () => {
      failures += 1;
      return Promise.reject(new Error("boom"));
    };
    expect(await diagrams.draw("bad", failing)).toBeNull();
    expect(await diagrams.draw("bad", failing)).toBeNull();
    expect(failures).toBe(2);
    expect(await diagrams.draw("none", () => Promise.resolve(null))).toBeNull();
    // a failure does not block the ones queued after it
    expect(await diagrams.draw("after", () => Promise.resolve("x"))).toBe("x");
  });
});

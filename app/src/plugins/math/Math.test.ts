// @vitest-environment jsdom
// Chemistry in formulas: `\ce` and `\pu` are drawn by KaTeX like any other formula.

import { flushSync, mount, tick, unmount } from "svelte";
import { afterEach, describe, expect, it } from "vitest";
import MathFormula from "./Math.svelte";

let mounted: ReturnType<typeof mount> | null = null;

afterEach(() => {
  if (mounted) void unmount(mounted);
  mounted = null;
  document.body.replaceChildren();
});

async function drawn(source: string, display = false): Promise<HTMLElement> {
  const target = document.createElement("div");
  document.body.append(target);
  mounted = mount(MathFormula, { target, props: { source, info: "", display } });
  for (let i = 0; i < 200; i++) {
    if (target.querySelector(".katex, .pm-math-error")) return target;
    await new Promise((resolve) => setTimeout(resolve, 10));
    flushSync();
    await tick();
  }
  throw new Error(`not drawn: ${source}`);
}

describe("drawing", () => {
  // the reading view measures a table again once an embed mounts in one of its cells: a formula that drew later
  // would leave the columns measured on its TeX source, far wider than the formula
  it("draws the formula as it mounts, never its source first", () => {
    const target = document.createElement("div");
    document.body.append(target);
    mounted = mount(MathFormula, {
      target,
      props: { source: "\\mathrm{pH_T}", info: "", display: false },
    });
    // what is laid out (the MathML copy keeps the source as an annotation, unseen)
    expect(target.querySelector(".katex-html")?.textContent).toContain("pH");
    expect(target.querySelector(".katex-html")?.textContent).not.toContain("\\mathrm");
  });
});

describe("chemistry formulas", () => {
  it.each([
    ["\\ce{H2O}", "H"],
    ["\\ce{A -> B}", "A"],
    ["\\ce{CO2 + C -> 2 CO}", "CO"],
    ["\\ce{Hg^2+ ->[I-] HgI2 ->[I-] [Hg^{II}I4]^2-}", "Hg"],
    ["\\pu{123 kJ/mol}", "kJ"],
  ])("%s is drawn, not marked as an error", async (source, seen) => {
    const target = await drawn(source);
    expect(target.querySelector(".pm-math-error")).toBeNull();
    expect(target.querySelector(".katex")?.textContent).toContain(seen);
  });

  it("an unknown command still shows as an error", async () => {
    const target = await drawn("\\nocommand{x}");
    expect(target.querySelector(".pm-math-error")).not.toBeNull();
  });
});

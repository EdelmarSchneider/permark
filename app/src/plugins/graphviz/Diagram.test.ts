// @vitest-environment jsdom
// The Graphviz block in the page: drawn as a figure, or, when the graph is wrong, the shared note with the engine's
// words and the source as plain text.

import { flushSync, mount, tick, unmount } from "svelte";
import { afterEach, describe, expect, it } from "vitest";
import { diagrams } from "../../core/doc/diagram";
import Diagram from "./Diagram.svelte";

let mounted: ReturnType<typeof mount> | null = null;

afterEach(() => {
  if (mounted) void unmount(mounted);
  mounted = null;
  document.body.replaceChildren();
  diagrams.clear();
});

async function until(check: () => boolean, what: string): Promise<void> {
  for (let i = 0; i < 200; i++) {
    if (check()) return;
    await new Promise((resolve) => setTimeout(resolve, 10));
    flushSync();
    await tick();
  }
  throw new Error(`timed out waiting for ${what}`);
}

function show(source: string): HTMLElement {
  const target = document.createElement("div");
  document.body.append(target);
  mounted = mount(Diagram, {
    target,
    props: { source, info: "dot", display: false, theme: "light" },
  });
  return target;
}

describe("a Graphviz block", () => {
  it("draws the graph", async () => {
    const target = show("digraph { a -> b }");
    await until(() => target.querySelector(".drawing svg") !== null, "drawing");
    expect(target.querySelector("figure")?.classList.contains("is-pending")).toBe(false);
  });

  it("a wrong graph shows the note with the engine's words; its source stays text", async () => {
    const target = show("digraph { a -> <b>");
    await until(() => target.querySelector(".figure.is-failed") !== null, "failure");
    const message = target.querySelector(".message")?.textContent ?? "";
    expect(message).toContain("Diagram could not be rendered");
    expect(message).toMatch(/syntax error in line 1/);
    expect(target.querySelector(".source b")).toBeNull();
    expect(target.querySelector(".source")?.textContent).toBe("digraph { a -> <b>");
  });
});

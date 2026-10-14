// @vitest-environment jsdom
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { EnhanceContext } from "../dom/enhance";

const fake = vi.hoisted(() => ({ renderFragments: vi.fn() }));
vi.mock("../api", () => ({ api: fake }));

import { createLockedHost } from "./lockedHost";

const context: EnhanceContext = {
  docPath: "docs/a.md",
  theme: "light",
  labels: {
    copyCode: "Copy",
    anchorLabel: (name) => name,
    diagramPending: "...",
    diagramFailed: "failed",
    alertTitles: {},
  },
  resolveImage: async (path) => path,
};

const labels = { edit: "Edit", editHint: "", region: (k: string) => k, reason: () => "" };

function setup() {
  const edits: [number | null, string][] = [];
  const links: string[] = [];
  const handle = createLockedHost({
    context: () => context,
    edit: (b, s) => edits.push([b, s]),
    link: (href) => links.push(href),
    labels,
  });
  const body = (): HTMLElement => document.createElement("div");
  return { handle, edits, links, body };
}

beforeEach(() => {
  fake.renderFragments.mockReset();
});

describe("the locked block host", () => {
  it("shows the source at once and the backend's HTML when it arrives, asking for the blocks that appeared together in one call", async () => {
    fake.renderFragments.mockResolvedValue(["<p>one</p>", "<table><tr><td>t</td></tr></table>"]);
    const { handle, body } = setup();
    const [a, b, a2] = [body(), body(), body()];
    handle.host.render(a, { kind: "html", source: "<b>one</b>", reason: "kind" });
    handle.host.render(b, { kind: "table", source: "| t |", reason: "differs" });
    // the same source twice is one request
    handle.host.render(a2, { kind: "html", source: "<b>one</b>", reason: "kind" });
    expect(a.textContent).toBe("<b>one</b>");
    expect(a.querySelector("pre.tm-locked__source")).not.toBeNull();
    await vi.waitFor(() => expect(a.querySelector("p")?.textContent).toBe("one"));
    expect(fake.renderFragments).toHaveBeenCalledTimes(1);
    expect(fake.renderFragments).toHaveBeenCalledWith(["<b>one</b>", "| t |"]);
    expect(a2.querySelector("p")?.textContent).toBe("one");
    expect(b.querySelector("table")).not.toBeNull();
  });

  it("draws a block it has already seen without asking again", async () => {
    fake.renderFragments.mockResolvedValue(["<p>once</p>"]);
    const { handle, body } = setup();
    handle.host.render(body(), { kind: "html", source: "x", reason: "kind" });
    await vi.waitFor(() => expect(fake.renderFragments).toHaveBeenCalledTimes(1));
    await Promise.resolve();
    const again = body();
    handle.host.render(again, { kind: "html", source: "x", reason: "kind" });
    expect(again.querySelector("p")?.textContent).toBe("once");
    expect(fake.renderFragments).toHaveBeenCalledTimes(1);
  });

  it("draws a block that uses a reference together with the definitions it uses", async () => {
    fake.renderFragments.mockResolvedValue(['<p>See <a href="/a">x</a></p>']);
    const { handle, body } = setup();
    const el = body();
    handle.host.render(el, {
      kind: "other",
      source: "See [x][ref]",
      reason: "reference",
      definitions: "[ref]: </a>",
    });
    // while the HTML is on its way the block shows its own source, not the definitions
    expect(el.textContent).toBe("See [x][ref]");
    await vi.waitFor(() => expect(el.querySelector("a")?.textContent).toBe("x"));
    expect(fake.renderFragments).toHaveBeenCalledWith(["See [x][ref]\n\n[ref]: </a>"]);
  });

  it("keeps front matter as text, without asking the backend", () => {
    const { handle, body } = setup();
    const el = body();
    handle.host.render(el, { kind: "frontmatter", source: "---\ntitle: t\n---", reason: "kind" });
    expect(el.querySelector("pre")?.textContent).toBe("---\ntitle: t\n---");
    expect(fake.renderFragments).not.toHaveBeenCalled();
  });

  it("keeps the source when the backend has no HTML for a block, or fails", async () => {
    fake.renderFragments.mockResolvedValueOnce([""]);
    const { handle, body } = setup();
    const empty = body();
    handle.host.render(empty, { kind: "html", source: "huge", reason: "kind" });
    await vi.waitFor(() => expect(fake.renderFragments).toHaveBeenCalledTimes(1));
    await Promise.resolve();
    expect(empty.textContent).toBe("huge");

    fake.renderFragments.mockRejectedValueOnce(new Error("down"));
    const failed = body();
    handle.host.render(failed, { kind: "html", source: "other", reason: "kind" });
    await vi.waitFor(() => expect(fake.renderFragments).toHaveBeenCalledTimes(2));
    await Promise.resolve();
    expect(failed.textContent).toBe("other");
  });

  it("passes the button on to the editor, and stops drawing after dispose", async () => {
    fake.renderFragments.mockResolvedValue(["<p>late</p>"]);
    const { handle, edits, body } = setup();
    handle.host.edit(3, "src");
    expect(edits).toEqual([[3, "src"]]);
    const el = body();
    handle.host.render(el, { kind: "html", source: "late", reason: "kind" });
    handle.dispose();
    await Promise.resolve();
    await Promise.resolve();
    expect(el.querySelector("p")).toBeNull();
  });
});

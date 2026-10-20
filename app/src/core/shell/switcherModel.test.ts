import { describe, expect, it } from "vitest";
import type { WindowView, WorkspaceSummary } from "../../api";
import {
  afterClose,
  canSwitch,
  chipsTight,
  hueOf,
  neighbor,
  shownId,
  splitName,
} from "./switcherModel";

function w(id: number, kind: WorkspaceSummary["kind"] = "project"): WorkspaceSummary {
  return { id, kind, title: `w${id}`, path: `/w${id}`, windows: 1 };
}

describe("the switcher bar", () => {
  it("switching needs two or more workspaces", () => {
    expect(canSwitch([])).toBe(false);
    expect(canSwitch([w(1)])).toBe(false);
    expect(canSwitch([w(1), w(2)])).toBe(true);
  });

  it("next | previous wrap around the list", () => {
    const list = [w(1), w(2), w(3)];
    expect(neighbor(list, 1, 1)?.id).toBe(2);
    expect(neighbor(list, 3, 1)?.id).toBe(1);
    expect(neighbor(list, 1, -1)?.id).toBe(3);
    expect(neighbor(list, 2, -1)?.id).toBe(1);
  });

  it("a window that shows nothing goes to the first (next) or the last (previous)", () => {
    const list = [w(1), w(2), w(3)];
    expect(neighbor(list, null, 1)?.id).toBe(1);
    expect(neighbor(list, null, -1)?.id).toBe(3);
    // a workspace that is not in the list (just closed) is the same as none
    expect(neighbor(list, 99, 1)?.id).toBe(1);
  });

  it("with one workspace or none there is nowhere to go", () => {
    expect(neighbor([], null, 1)).toBeNull();
    expect(neighbor([w(1)], 1, 1)).toBeNull();
  });

  it("the workspace a window shows, also while another opens", () => {
    const project = {
      kind: "project",
      id: 5,
      name: "p",
      root: "/p",
      stateLocation: "profile",
    } as const;
    const showing: WindowView = { state: "showing", workspace: project, document: null };
    expect(shownId(showing)).toBe(5);
    const target = { kind: "notes", path: null } as const;
    expect(shownId({ state: "opening", target, showing: project })).toBe(5);
    expect(shownId({ state: "opening", target, showing: null })).toBeNull();
    expect(shownId({ state: "welcome" })).toBeNull();
    expect(shownId({ state: "closing" })).toBeNull();
  });
});

describe("after closing a workspace", () => {
  it("the next one in the list takes its place, else the one before", () => {
    const list = [w(1), w(2), w(3)];
    expect(afterClose(list, 1)?.id).toBe(2);
    expect(afterClose(list, 2)?.id).toBe(3);
    expect(afterClose(list, 3)?.id).toBe(2);
  });

  it("nothing when it was the last one; the first when it is not listed (already gone)", () => {
    expect(afterClose([w(1)], 1)).toBeNull();
    expect(afterClose([], 1)).toBeNull();
    expect(afterClose([w(4), w(5)], 9)?.id).toBe(4);
  });
});

describe("a chip's hue", () => {
  const project = (path: string) => ({ kind: "project" as const, path });

  it("Notes are yellow, a single file gray, a project one of five calm hues", () => {
    expect(hueOf({ kind: "notes", path: "" })).toBe("yellow");
    expect(hueOf({ kind: "singleFile", path: "C:/a.md" })).toBe("gray");
    const hues = new Set(
      Array.from({ length: 200 }, (_, i) => hueOf(project(`C:/work/project-${i}`))),
    );
    expect([...hues].sort()).toEqual(["blue", "green", "orange", "purple", "teal"]);
  });

  it("is stable: same folder, same hue, whatever the case or the separator", () => {
    const a = hueOf(project(String.raw`C:\Work\Atlas`));
    expect(hueOf(project("c:/work/atlas"))).toBe(a);
    expect(hueOf(project("C:/WORK/ATLAS"))).toBe(a);
    // odd paths still give a hue
    for (const path of ["", "/", "\u0000", "Projeto ação 日本", "🙂".repeat(500)]) {
      expect(["blue", "teal", "green", "purple", "orange"]).toContain(hueOf(project(path)));
    }
  });
});

describe("a long name cut in the middle", () => {
  it("short names stay whole", () => {
    expect(splitName("atlas", false)).toEqual({ head: "atlas", tail: "" });
    expect(splitName("fourteen-chars", false)).toEqual({ head: "fourteen-chars", tail: "" });
    expect(splitName("", true)).toEqual({ head: "", tail: "" });
  });

  it("a folder keeps its last letters; a file its extension and a few letters before it", () => {
    expect(splitName("product-specs-and-concepts-archive", false)).toEqual({
      head: "product-specs-and-concepts-a",
      tail: "rchive",
    });
    expect(splitName("proposal-for-the-quarterly-budget-review.md", true)).toEqual({
      head: "proposal-for-the-quarterly-budget-re",
      tail: "view.md",
    });
    // a dot that is not an extension (too long, or the name starts with it) keeps six letters
    expect(splitName("archive.version-two-thousand", true).tail).toBe("ousand");
    expect(splitName(".gitignore-of-the-project", true).tail).toBe("roject");
  });

  it("never splits a character: accents, CJK, emoji", () => {
    const name = "Projeto ação 日本 🙂🙂 relatório final";
    const { head, tail } = splitName(name, false);
    expect(head + tail).toBe(name);
    expect(Array.from(tail)).toHaveLength(6);
    const emoji = splitName("🙂".repeat(20) + ".md", true);
    expect(emoji.head + emoji.tail).toBe("🙂".repeat(20) + ".md");
    expect(emoji.tail).toBe("🙂".repeat(4) + ".md");
  });
});

describe("many chips in little room", () => {
  it("names keep their start only below 150 px a chip; never with no chips or no room measured yet", () => {
    expect(chipsTight(1184, 5)).toBe(false);
    expect(chipsTight(1184, 12)).toBe(true);
    expect(chipsTight(600, 4)).toBe(false);
    expect(chipsTight(600, 5)).toBe(true);
    expect(chipsTight(0, 0)).toBe(false);
    expect(chipsTight(-96, 2)).toBe(true);
  });
});

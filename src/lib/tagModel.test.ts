import { describe, expect, it } from "vitest";
import {
  countTags,
  filterTags,
  isEmptySelection,
  knownTagsHint,
  matchTag,
  NO_SELECTION,
  pruneSelection,
  rankTags,
  selectionLabel,
  selectOnly,
  suggest,
  toggleTag,
  toggleUntagged,
  treeTooltip,
  withAppTags,
} from "./tagModel";
import type { FileMeta, IndexEntry } from "./types";

const tag = (name: string, count: number) => ({ name, count });
const TAGS = [
  tag("spec", 12),
  tag("needs-review", 3),
  tag("feed", 5),
  tag("watcher", 4),
  tag("watcher-api", 4),
  tag("api/v2", 1),
];

describe("matchTag", () => {
  it("tiers: whole name, start, word start, inside, characters in order", () => {
    expect(matchTag("spec", "spec")?.tier).toBe(0);
    expect(matchTag("sp", "spec")?.tier).toBe(1);
    expect(matchTag("rev", "needs-review")?.tier).toBe(2);
    expect(matchTag("v2", "api/v2")?.tier).toBe(2);
    expect(matchTag("eed", "needs-review")?.tier).toBe(3);
    expect(matchTag("nrv", "needs-review")?.tier).toBe(4);
  });

  it("returns the matched positions for highlighting", () => {
    expect(matchTag("rev", "needs-review")?.positions).toEqual([6, 7, 8]);
    expect(matchTag("nrv", "needs-review")?.positions).toEqual([0, 6, 8]);
  });

  it("does not match when characters are missing or out of order", () => {
    expect(matchTag("xyz", "spec")).toBeNull();
    expect(matchTag("ecps", "spec")).toBeNull();
  });

  it("ignores case and accents", () => {
    expect(matchTag("RÉV", "review")?.tier).toBe(1);
  });
});

describe("rankTags", () => {
  it("lists every tag by file count for an empty query", () => {
    expect(rankTags("", TAGS).map((m) => m.tag.name)).toEqual([
      "spec",
      "feed",
      "watcher",
      "watcher-api",
      "needs-review",
      "api/v2",
    ]);
  });

  it("puts better matches first and breaks ties with the file count", () => {
    expect(rankTags("wat", TAGS).map((m) => m.tag.name)).toEqual(["watcher", "watcher-api"]);
    // "api" starts "api/v2" (tier 1) and is a word of "watcher-api" (tier 2)
    expect(rankTags("api", TAGS).map((m) => m.tag.name)).toEqual(["api/v2", "watcher-api"]);
    // equal tier: the one on more files wins
    expect(
      rankTags("e", TAGS)
        .map((m) => m.tag.name)
        .slice(0, 2),
    ).toEqual(["spec", "feed"]);
  });

  it("leaves out excluded tags", () => {
    expect(rankTags("wat", TAGS, new Set(["watcher"])).map((m) => m.tag.name)).toEqual([
      "watcher-api",
    ]);
  });
});

describe("filterTags", () => {
  it("keeps the order by file count", () => {
    expect(filterTags(TAGS, "w").map((t) => t.name)).toEqual([
      "needs-review",
      "watcher",
      "watcher-api",
    ]);
    expect(filterTags(TAGS, "  ")).toEqual(TAGS);
    expect(filterTags(TAGS, "zzz")).toEqual([]);
  });
});

describe("suggest", () => {
  const none = new Set<string>();

  it("offers matches and a create row for a new name", () => {
    const rows = suggest("wat", TAGS, none);
    expect(rows.map((r) => (r.type === "tag" ? r.match.tag.name : `create:${r.name}`))).toEqual([
      "watcher",
      "watcher-api",
      "create:wat",
    ]);
  });

  it("has no create row when the text already is a tag", () => {
    const rows = suggest("watcher", TAGS, none);
    expect(rows.some((r) => r.type === "create")).toBe(false);
    expect(rows[0]).toMatchObject({ type: "tag" });
  });

  it("has no create row when the file already has that tag", () => {
    expect(suggest("Mine", TAGS, new Set(["mine"])).some((r) => r.type === "create")).toBe(false);
  });

  it("normalizes the name to create", () => {
    expect(suggest(" Brand New ", [], none)).toEqual([{ type: "create", name: "brand-new" }]);
  });

  it("offers the top tags for empty text", () => {
    const rows = suggest("", TAGS, none, 3);
    expect(rows).toHaveLength(3);
    expect(rows.every((r) => r.type === "tag")).toBe(true);
  });

  it("does not offer the tags the file has", () => {
    const rows = suggest("spec", TAGS, new Set(["spec"]));
    expect(rows.filter((r) => r.type === "tag")).toHaveLength(0);
  });
});

describe("selection", () => {
  it("a click selects only that tag", () => {
    expect(selectOnly("spec")).toEqual({ tags: ["spec"], untagged: false });
  });

  it("toggling adds and removes, keeping the others", () => {
    let sel = selectOnly("spec");
    sel = toggleTag(sel, "feed");
    expect(sel.tags).toEqual(["spec", "feed"]);
    sel = toggleTag(sel, "spec");
    expect(sel.tags).toEqual(["feed"]);
    expect(isEmptySelection(toggleTag(sel, "feed"))).toBe(true);
  });

  it("untagged excludes tags, and tags exclude untagged", () => {
    const sel = toggleUntagged(selectOnly("spec"), true);
    expect(sel).toEqual({ tags: [], untagged: true });
    expect(toggleTag(sel, "feed")).toEqual({ tags: ["feed"], untagged: false });
    expect(toggleUntagged(sel, false)).toEqual(NO_SELECTION);
    expect(toggleUntagged(NO_SELECTION, false)).toEqual({ tags: [], untagged: true });
  });

  it("is labelled by its tags", () => {
    expect(selectionLabel({ tags: ["spec", "needs-review"], untagged: false }, "Untagged")).toBe(
      "spec + needs-review",
    );
    expect(selectionLabel({ tags: [], untagged: true }, "Untagged")).toBe("Untagged");
  });

  it("drops tags that no longer exist", () => {
    const sel = { tags: ["spec", "gone"], untagged: false };
    expect(pruneSelection(sel, TAGS).tags).toEqual(["spec"]);
    const alive = [...TAGS, tag("gone", 1)];
    expect(pruneSelection(sel, alive)).toBe(sel);
  });
});

describe("countTags and knownTagsHint", () => {
  const entry = (path: string, tags: string[]): IndexEntry => ({
    path,
    tags,
    description: "",
    modifiedMs: 0,
  });

  it("counts files per tag, most files first", () => {
    const counts = countTags([entry("a", ["x", "y"]), entry("b", ["y"]), entry("c", [])]);
    expect(counts).toEqual([tag("y", 2), tag("x", 1)]);
  });

  it("names the first few tags", () => {
    expect(knownTagsHint(TAGS.slice(0, 3))).toBe("spec, needs-review, feed");
    expect(knownTagsHint(TAGS, 2)).toBe("spec, needs-review...");
    expect(knownTagsHint([])).toBe("");
  });
});

describe("treeTooltip", () => {
  // the line is worded by the caller (a translated string with the list in it)
  const line = (list: string): string => `Tags: ${list}`;

  it("adds the description and the tags line below the path", () => {
    expect(treeTooltip("a/b.md", { tags: ["spec", "feed"], description: "About b" }, line)).toBe(
      "a/b.md\nAbout b\nTags: spec, feed",
    );
    expect(treeTooltip("a/b.md", { tags: [], description: "Only text" }, line)).toBe(
      "a/b.md\nOnly text",
    );
    expect(treeTooltip("a/b.md", { tags: ["x"], description: "" }, line)).toBe("a/b.md\nTags: x");
  });

  it("leaves the tooltip alone for a file without either", () => {
    expect(treeTooltip("a/b.md", undefined, line)).toBeUndefined();
    expect(treeTooltip("a/b.md", { tags: [], description: "" }, line)).toBeUndefined();
  });

  it("lets the caller put the list anywhere in the line", () => {
    const reordered = (list: string): string => `${list} <- tags`;
    expect(treeTooltip("p.md", { tags: ["a", "b"], description: "" }, reordered)).toBe(
      "p.md\na, b <- tags",
    );
  });
});

describe("withAppTags", () => {
  const meta: FileMeta = {
    path: "a.md",
    description: "d",
    writable: true,
    tags: [
      { name: "feed", file: true, app: false },
      { name: "watcher", file: true, app: true },
      { name: "spec", file: false, app: true },
    ],
  };

  it("keeps the front matter's tags and follows the new list for the app's", () => {
    const next = withAppTags(meta, ["spec", "needs-review"]);
    expect(next.tags).toEqual([
      { name: "feed", file: true, app: false },
      { name: "watcher", file: true, app: false },
      { name: "spec", file: false, app: true },
      { name: "needs-review", file: false, app: true },
    ]);
    expect(next.description).toBe("d");
  });

  it("shows a tag the file already has once, as a file tag, marking that the app has it too", () => {
    const next = withAppTags(meta, ["feed"]);
    expect(next.tags).toEqual([
      { name: "feed", file: true, app: true },
      { name: "watcher", file: true, app: false },
    ]);
  });

  it("normalizes the names and leaves the original alone", () => {
    const next = withAppTags(meta, [" New Idea ", "NEW IDEA"]);
    expect(next.tags.filter((t) => !t.file)).toEqual([
      { name: "new-idea", file: false, app: true },
    ]);
    expect(meta.tags).toHaveLength(3);
  });

  it("two changes in a row build on each other", () => {
    const first = withAppTags(meta, ["spec", "one"]);
    const appNames = first.tags.filter((t) => t.app).map((t) => t.name);
    const second = withAppTags(first, [...appNames, "two"]);
    expect(second.tags.filter((t) => t.app && !t.file).map((t) => t.name)).toEqual([
      "spec",
      "one",
      "two",
    ]);
  });
});

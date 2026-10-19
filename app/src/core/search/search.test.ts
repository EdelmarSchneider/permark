// Pure parts of search in the window: fuzzy match, the Search everywhere model, the panel's grouping, recent files.
import { describe, expect, it } from "vitest";
import type { FileHit, FolderHit, OutlineItem, Recent, SectionHit } from "../../api";
import { NO_REQUIREMENT } from "../conditions/available";
import type { CommandEntry } from "../commands/commands";
import {
  GROUP_LIMIT,
  NO_DATA,
  applyPrefix,
  buildView,
  filterOutline,
  firstScope,
  moveActive,
  outlineRows,
  scopesFor,
  stepScope,
  type Data,
  type ViewInput,
} from "./finderModel";
import { fuzzyMatch, markRuns } from "./fuzzy";
import { flattenRows, groupHits } from "./panelModel";
import { MAX_RECENT, bumped, parseRecent } from "./recent.svelte";

describe("fuzzy match", () => {
  it("ranks name prefix > name substring > name subsequence > path substring > path subsequence", () => {
    const score = (path: string): number => fuzzyMatch("plan", path)?.score ?? -1;
    const tiers = [
      "docs/plan.md",
      "docs/my-plan.md",
      "docs/p-l-a-n.md",
      "plan/readme.md",
      "p/l/a/n.md",
    ].map(score);
    for (let i = 1; i < tiers.length; i++) expect(tiers[i - 1]).toBeGreaterThan(tiers[i] ?? 0);
    expect(fuzzyMatch("plan", "docs/other.md")).toBeNull();
    expect(fuzzyMatch("   ", "docs/plan.md")).toBeNull();
  });

  it("reports the characters it matched, aligned with accents and astral characters", () => {
    expect(fuzzyMatch("rp", "docs/retry-policy.md")?.positions).toEqual([5, 11]);
    const text = "Índice 💥 final";
    const found = fuzzyMatch("indice", text);
    expect(found?.positions).toEqual([0, 1, 2, 3, 4, 5]);
    const runs = markRuns(text, found?.positions ?? []);
    expect(runs[0]).toEqual({ text: "Índice", mark: true });
    expect(runs.map((r) => r.text).join("")).toBe(text);
  });

  it("matches several words in any order", () => {
    expect(fuzzyMatch("token auth", "docs/auth-flow/token.md")).not.toBeNull();
    expect(fuzzyMatch("token missing", "docs/auth-flow/token.md")).toBeNull();
  });

  it("never throws on odd input", () => {
    for (const q of ["\0", "💥", "////", "a b c d e f g", "é"]) {
      fuzzyMatch(q, "dir/💥 file é.md");
      fuzzyMatch(q, "");
    }
  });
});

const file = (path: string, positions: number[] = []): FileHit => ({
  path,
  category: "markdown",
  positions,
});
const folder = (path: string): FolderHit => ({ path, positions: [] });
const section = (path: string, line: number, heading = "H"): SectionHit => ({
  path,
  headingPath: [heading],
  anchor: heading.toLowerCase(),
  line,
  snippet: [{ text: heading, mark: false }],
  score: 1,
});
function command(id: string): CommandEntry {
  return {
    command: {
      kind: "command",
      owner: "core",
      id,
      label: "cmd.save",
      run: () => {},
      requires: NO_REQUIREMENT,
    },
    combos: [],
    availability: { status: "available", degraded: [] },
  };
}

function input(over: Omit<Partial<ViewInput>, "data"> & { data?: Partial<Data> }): ViewInput {
  const { data, ...rest } = over;
  return {
    preset: "everywhere",
    scope: "all",
    query: "",
    data: { ...NO_DATA, ...data },
    commands: [],
    outline: [],
    singleFile: false,
    ...rest,
  };
}

describe("the scopes of the box", () => {
  it("each face offers its own", () => {
    expect(scopesFor("quick", false)).toEqual(["files"]);
    expect(scopesFor("commands", false)).toEqual(["commands"]);
    expect(scopesFor("everywhere", false)).toEqual(["all", "files", "sections", "commands"]);
    expect(scopesFor("everywhere", true)).toEqual(["sections", "commands"]);
    expect(firstScope(scopesFor("everywhere", true))).toBe("sections");
  });

  it("Tab walks them and wraps", () => {
    const scopes = scopesFor("everywhere", false);
    expect(stepScope(scopes, "all", 1)).toBe("files");
    expect(stepScope(scopes, "commands", 1)).toBe("all");
    expect(stepScope(scopes, "all", -1)).toBe("commands");
  });

  it("a first character `>` or `#` switches the scope and leaves the text, when offered", () => {
    const scopes = scopesFor("everywhere", false);
    expect(applyPrefix(scopes, "all", ">save")).toEqual({ scope: "commands", text: "save" });
    expect(applyPrefix(scopes, "all", "#intro")).toEqual({ scope: "sections", text: "intro" });
    expect(applyPrefix(scopes, "all", "plain")).toEqual({ scope: "all", text: "plain" });
    expect(applyPrefix(scopes, "all", "a>b")).toEqual({ scope: "all", text: "a>b" });
    // a face with one scope: the character is text
    expect(applyPrefix(["files"], "files", ">x")).toEqual({ scope: "files", text: ">x" });
    // single file: sections and commands only, both prefixes work
    expect(applyPrefix(scopesFor("everywhere", true), "sections", ">x").scope).toBe("commands");
  });
});

describe("what each scope lists", () => {
  const recent: Recent = {
    opened: [file("a.md"), file("b.md")],
    changed: [file("b.md"), file("c.md")],
  };

  it("an empty All or Files box lists recent files once each: opened first, then changed", () => {
    for (const scope of ["all", "files"] as const) {
      const view = buildView(input({ scope, data: { recent } }));
      expect(view.groups.map((g) => g.title)).toEqual(["opened", "changed"]);
      expect(view.rows.map((r) => (r.type === "file" ? r.path : ""))).toEqual([
        "a.md",
        "b.md",
        "c.md",
      ]);
      expect(view.empty).toBe("none");
    }
  });

  it("All groups results in a fixed order, caps each group and offers `Show all`", () => {
    const files = Array.from({ length: 8 }, (_, i) => file(`f${i}.md`));
    const view = buildView(
      input({
        query: "x",
        data: {
          files,
          folders: [folder("d1")],
          sections: [section("a.md", 3)],
        },
        commands: [{ entry: command("edit.save"), positions: [] }],
      }),
    );
    expect(view.groups.map((g) => g.title)).toEqual(["files", "folders", "sections", "commands"]);
    const filesGroup = view.groups[0]?.rows ?? [];
    expect(filesGroup).toHaveLength(GROUP_LIMIT.files + 1);
    expect(filesGroup.at(-1)).toMatchObject({ type: "more", scope: "files", count: 3 });
  });

  it("a group without results is left out and a typed query with nothing says so", () => {
    const only = buildView(input({ query: "x", data: { sections: [section("a.md", 1)] } }));
    expect(only.groups.map((g) => g.title)).toEqual(["sections"]);
    const none = buildView(input({ query: "nothing" }));
    expect(none.rows).toEqual([]);
    expect(none.empty).toBe("noMatch");
    expect(buildView(input({ query: "" })).empty).toBe("none");
  });

  it("Quick open lists files only, without folders", () => {
    const view = buildView(
      input({
        preset: "quick",
        scope: "files",
        query: "x",
        data: { files: [file("a.md")], folders: [folder("d")] },
      }),
    );
    expect(view.groups.map((g) => g.title)).toEqual(["files"]);
    const wide = buildView(
      input({
        scope: "files",
        query: "x",
        data: { files: [file("a.md")], folders: [folder("d")] },
      }),
    );
    expect(wide.groups.map((g) => g.title)).toEqual(["files", "folders"]);
  });

  it("Sections lists the headings of the open document before anything is typed, then the hits", () => {
    const outline: OutlineItem[] = [
      { label: "Top", level: 1, id: "top", line: 1, block: "b1" },
      { label: "Deep", level: 2, id: "deep", line: 5, block: "b2" },
      { label: "Next", level: 1, id: "next", line: 9, block: "b3" },
    ];
    const rows = outlineRows("doc.md", outline);
    expect(rows.map((r) => r.headingPath)).toEqual([["Top"], ["Top", "Deep"], ["Next"]]);
    const empty = buildView(input({ scope: "sections", outline: rows }));
    expect(empty.rows).toHaveLength(3);
    const typed = buildView(
      input({
        scope: "sections",
        query: "x",
        outline: rows,
        data: { sections: [section("z.md", 2)] },
      }),
    );
    expect(typed.rows.map((r) => (r.type === "section" ? r.path : ""))).toEqual(["z.md"]);
    // a single file filters the outline itself
    const local = buildView(
      input({ scope: "sections", query: "dee", outline: rows, singleFile: true }),
    );
    expect(local.rows.map((r) => (r.type === "section" ? r.anchor : ""))).toEqual(["deep"]);
    expect(filterOutline(rows, "zzz")).toEqual([]);
  });

  it("Commands lists the matched commands", () => {
    const view = buildView(
      input({
        preset: "commands",
        scope: "commands",
        commands: [
          { entry: command("a.one"), positions: [] },
          { entry: command("b.two"), positions: [] },
        ],
      }),
    );
    expect(view.rows.map((r) => (r.type === "command" ? r.entry.command.id : ""))).toEqual([
      "a.one",
      "b.two",
    ]);
  });

  it("the keyboard wraps around the ends", () => {
    expect(moveActive(0, -1, 3)).toBe(2);
    expect(moveActive(2, 1, 3)).toBe(0);
    expect(moveActive(0, 1, 0)).toBe(0);
  });
});

describe("the Search panel's results", () => {
  const hits = [section("b.md", 4, "X"), section("a.md", 1, "Y"), section("b.md", 9, "Z")];

  it("groups by document in the order of the best hit, keeping each document's ranking", () => {
    const groups = groupHits(hits);
    expect(groups.map((g) => g.path)).toEqual(["b.md", "a.md"]);
    expect(groups[0]?.hits.map((h) => h.line)).toEqual([4, 9]);
  });

  it("flattens to header + hits, dropping the hits of a folded document", () => {
    const groups = groupHits(hits);
    expect(flattenRows(groups, new Set()).map((r) => r.type)).toEqual([
      "file",
      "hit",
      "hit",
      "file",
      "hit",
    ]);
    expect(flattenRows(groups, new Set(["b.md"])).map((r) => r.type)).toEqual([
      "file",
      "file",
      "hit",
    ]);
    expect(flattenRows([], new Set())).toEqual([]);
  });
});

describe("recent files", () => {
  it("reads only strings, once each, at most the limit", () => {
    expect(parseRecent(["a", "a", 3, "", "b", null])).toEqual(["a", "b"]);
    expect(parseRecent("nope")).toEqual([]);
    expect(parseRecent(Array.from({ length: 100 }, (_, i) => `f${i}`))).toHaveLength(MAX_RECENT);
  });

  it("puts the newest first and keeps the list short", () => {
    expect(bumped(["a", "b", "c"], "c")).toEqual(["c", "a", "b"]);
    expect(bumped(["a"], "z")).toEqual(["z", "a"]);
    const long = Array.from({ length: MAX_RECENT }, (_, i) => `f${i}`);
    const next = bumped(long, "new");
    expect(next).toHaveLength(MAX_RECENT);
    expect(next[0]).toBe("new");
    expect(next).not.toContain(`f${MAX_RECENT - 1}`);
  });
});

describe("the Tags scope of Search everywhere", () => {
  const input = (
    scope: ViewInput["scope"],
    query: string,
    over: Partial<ViewInput> = {},
  ): ViewInput => ({
    preset: "everywhere",
    scope,
    query,
    data: NO_DATA,
    commands: [],
    outline: [],
    singleFile: false,
    tags: [
      { name: "spec", count: 5 },
      { name: "needs-review", count: 2 },
    ],
    ...over,
  });

  it("is offered with the tags plugin, between Sections and Commands", () => {
    expect(scopesFor("everywhere", false, true)).toEqual([
      "all",
      "files",
      "sections",
      "tags",
      "commands",
    ]);
    expect(scopesFor("everywhere", false)).toEqual(["all", "files", "sections", "commands"]);
    expect(scopesFor("everywhere", true, true)).toEqual(["sections", "commands"]);
    expect(scopesFor("quick", false, true)).toEqual(["files"]);
    expect(scopesFor("commands", false, true)).toEqual(["commands"]);
  });

  it("lists the tags that match, with their counts, in its own scope and as a group in All", () => {
    const own = buildView(input("tags", ""));
    expect(own.groups.map((g) => g.title)).toEqual(["tags"]);
    expect(own.rows.map((r) => (r.type === "tag" ? [r.name, r.count] : null))).toEqual([
      ["spec", 5],
      ["needs-review", 2],
    ]);
    const all = buildView(input("all", "spec"));
    expect(all.groups.map((g) => g.title)).toEqual(["tags"]);
    // an empty box in All shows recent files, not every tag
    expect(buildView(input("all", "")).groups).toEqual([]);
  });

  it("keeps the rows of All in the order files, folders, sections, tags, commands", () => {
    const data: Data = {
      files: [{ path: "a.md", category: "markdown", positions: [0] }],
      folders: [{ path: "docs", positions: [0] }],
      sections: [],
      recent: null,
    };
    const view = buildView(input("all", "a", { data }));
    expect(view.groups.map((g) => g.title)).toEqual(["files", "folders", "tags"]);
  });

  it("caps the group in All and sends the rest to the scope", () => {
    const tags = Array.from({ length: 9 }, (_, n) => ({ name: `t${n}`, count: 9 - n }));
    const view = buildView(input("all", "t", { tags }));
    const rows = view.groups[0]?.rows ?? [];
    expect(rows).toHaveLength(GROUP_LIMIT.tags + 1);
    expect(rows.at(-1)).toMatchObject({ type: "more", scope: "tags", count: 4 });
  });
});

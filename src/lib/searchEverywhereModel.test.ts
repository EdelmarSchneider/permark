import { describe, expect, it } from "vitest";
import type { CommandMatch } from "./commands";
import { COMMANDS } from "./commands";
import {
  allowedScope,
  applyPrefix,
  buildView,
  commandRows,
  fileRows,
  GROUP_LIMIT,
  moveActive,
  outlineRows,
  RECENT_LIMIT,
  recentFileRows,
  sectionRows,
  SCOPES,
  scopesFor,
  stepScope,
  tagRows,
  type CommandRow,
  type FileRow,
  type Row,
  type SectionRow,
  type TagRow,
  type ViewInput,
} from "./searchEverywhereModel";
import type { SearchHit } from "./types";

// ---- scopes and prefixes ------------------------------------------------------------------------------

describe("scopes", () => {
  it("offers all five with a folder open and only the commands without one", () => {
    expect(scopesFor(true)).toEqual(["all", "files", "sections", "tags", "commands"]);
    expect(scopesFor(false)).toEqual(["commands"]);
  });

  it("walks forward and backward, wrapping around the ends", () => {
    expect(stepScope(SCOPES, "all", 1)).toBe("files");
    expect(stepScope(SCOPES, "tags", 1)).toBe("commands");
    expect(stepScope(SCOPES, "commands", 1)).toBe("all");
    expect(stepScope(SCOPES, "all", -1)).toBe("commands");
    expect(stepScope(SCOPES, "sections", -1)).toBe("files");
  });

  it("stays put when there is a single scope", () => {
    const only = scopesFor(false);
    expect(stepScope(only, "commands", 1)).toBe("commands");
    expect(stepScope(only, "commands", -1)).toBe("commands");
  });

  it("falls back to the first scope when the asked one is not offered", () => {
    expect(allowedScope(scopesFor(false), "tags")).toBe("commands");
    expect(allowedScope(SCOPES, "tags")).toBe("tags");
  });
});

describe("applyPrefix", () => {
  it("switches to Commands on > and to Sections on #, removing the prefix", () => {
    expect(applyPrefix(">toggle", "all", SCOPES)).toEqual({ scope: "commands", text: "toggle" });
    expect(applyPrefix("#refresh token", "files", SCOPES)).toEqual({
      scope: "sections",
      text: "refresh token",
    });
  });

  it("drops the spaces typed after the prefix", () => {
    expect(applyPrefix(">  save", "all", SCOPES)).toEqual({ scope: "commands", text: "save" });
  });

  it("switches on the prefix alone, leaving an empty box", () => {
    expect(applyPrefix(">", "all", SCOPES)).toEqual({ scope: "commands", text: "" });
    expect(applyPrefix("#", "tags", SCOPES)).toEqual({ scope: "sections", text: "" });
  });

  it("only reads the first character", () => {
    expect(applyPrefix("a > b", "all", SCOPES)).toEqual({ scope: "all", text: "a > b" });
    expect(applyPrefix("c#", "files", SCOPES)).toEqual({ scope: "files", text: "c#" });
    expect(applyPrefix(" >x", "all", SCOPES)).toEqual({ scope: "all", text: " >x" });
  });

  it("leaves a text with no prefix, and the scope, alone", () => {
    expect(applyPrefix("auth", "tags", SCOPES)).toEqual({ scope: "tags", text: "auth" });
    expect(applyPrefix("", "files", SCOPES)).toEqual({ scope: "files", text: "" });
  });

  it("treats a prefix as text where its scope is not offered", () => {
    expect(applyPrefix("#note", "commands", scopesFor(false))).toEqual({
      scope: "commands",
      text: "#note",
    });
    expect(applyPrefix(">save", "commands", scopesFor(false)).text).toBe("save");
  });
});

// ---- rows ----------------------------------------------------------------------------------------------

const noDescription = (): string => "";

describe("recentFileRows", () => {
  const exists = new Set(["a.md", "b.md", "c.md", "d.md"]);

  it("lists the files that changed, then the ones opened that did not", () => {
    const rows = recentFileRows({
      changed: [
        { path: "b.md", atMs: 20 },
        { path: "a.md", atMs: 10 },
      ],
      opened: ["a.md", "c.md", "d.md"],
      exists,
      describe: noDescription,
    });
    expect(rows.map((r) => [r.path, r.group, r.ms])).toEqual([
      ["b.md", "changed", 20],
      ["a.md", "changed", 10],
      ["c.md", "opened", null],
      ["d.md", "opened", null],
    ]);
  });

  it("lists a file that changed twice once, at its latest change", () => {
    const rows = recentFileRows({
      changed: [
        { path: "a.md", atMs: 30 },
        { path: "b.md", atMs: 20 },
        { path: "a.md", atMs: 10 },
      ],
      opened: [],
      exists,
      describe: noDescription,
    });
    expect(rows.map((r) => [r.path, r.ms])).toEqual([
      ["a.md", 30],
      ["b.md", 20],
    ]);
  });

  it("leaves out files the filter does not list, and carries the description", () => {
    const rows = recentFileRows({
      changed: [{ path: "gone.md", atMs: 5 }],
      opened: ["gone.md", "a.md"],
      exists,
      describe: (p) => `about ${p}`,
    });
    expect(rows.map((r) => r.path)).toEqual(["a.md"]);
    expect(rows[0].description).toBe("about a.md");
  });

  it("lists at most eight of each kind", () => {
    const paths = Array.from({ length: 12 }, (_, i) => `f${i}.md`);
    const rows = recentFileRows({
      changed: paths.slice(0, 10).map((path, i) => ({ path, atMs: i })),
      opened: paths.slice(10).concat(Array.from({ length: 12 }, (_, i) => `o${i}.md`)),
      exists: new Set([...paths, ...Array.from({ length: 12 }, (_, i) => `o${i}.md`)]),
      describe: noDescription,
    });
    expect(rows.filter((r) => r.group === "changed")).toHaveLength(RECENT_LIMIT);
    expect(rows.filter((r) => r.group === "opened")).toHaveLength(RECENT_LIMIT);
  });
});

describe("fileRows", () => {
  it("carries the match, the description and when the file changed", () => {
    const rows = fileRows(
      [
        { path: "docs/auth.md", positions: [5, 6], descriptionPositions: [], hidden: false },
        { path: ".private/x.md", positions: [], descriptionPositions: [1], hidden: true },
      ],
      (p) => (p === "docs/auth.md" ? "Login" : ""),
      new Map([["docs/auth.md", 99]]),
    );
    expect(rows[0]).toMatchObject({
      type: "file",
      key: "file:docs/auth.md",
      positions: [5, 6],
      description: "Login",
      ms: 99,
    });
    expect(rows[0].hidden).toBeUndefined();
    expect(rows[1]).toMatchObject({ ms: null, hidden: true, descriptionPositions: [1] });
  });
});

function hit(over: Partial<SearchHit> & { path: string }): SearchHit {
  return {
    headingPath: [],
    snippet: "",
    startLine: 1,
    score: 1,
    ...over,
  };
}

describe("sectionRows", () => {
  const visible = new Set(["docs/auth.md", "docs/notes.md", "README.md", "@wt/feat/docs/auth.md"]);
  const base = { visible, tags: [], tagsOf: () => undefined, limit: 50 };

  it("names the section by its own heading, with the file and the headings above it", () => {
    const [row] = sectionRows({
      ...base,
      hits: [
        hit({
          path: "docs/auth.md",
          headingPath: ["Auth", "Login", "Refreshing the token"],
          snippet: "the access <mark>token</mark> expires",
          startLine: 42,
        }),
      ],
    });
    expect(row).toMatchObject({
      type: "section",
      key: "section:docs/auth.md:42",
      title: "Refreshing the token",
      where: "auth.md › Auth › Login",
      snippet: "the access <mark>token</mark> expires",
    });
    expect(row.hit?.startLine).toBe(42);
    expect(row.outline).toBeNull();
  });

  it("shows only the file for a top heading", () => {
    const [row] = sectionRows({
      ...base,
      hits: [hit({ path: "docs/auth.md", headingPath: ["Auth"] })],
    });
    expect(row.title).toBe("Auth");
    expect(row.where).toBe("auth.md");
  });

  it("names a hit above the first heading by its file, and says where the file is", () => {
    const [row] = sectionRows({ ...base, hits: [hit({ path: "docs/notes.md" })] });
    expect(row.title).toBe("notes.md");
    expect(row.where).toBe("docs");
  });

  it("keeps only the tags of a snippet's <mark>", () => {
    const [row] = sectionRows({
      ...base,
      hits: [hit({ path: "README.md", snippet: "<script>x</script> <mark>y</mark>" })],
    });
    expect(row.snippet).toBe("&lt;script>x&lt;/script> <mark>y</mark>");
  });

  it("leaves out the files the Explorer filter hides", () => {
    const rows = sectionRows({
      ...base,
      hits: [hit({ path: "hidden/x.md" }), hit({ path: "docs/auth.md", startLine: 3 })],
    });
    expect(rows.map((r) => r.path)).toEqual(["docs/auth.md"]);
  });

  it("keeps the hits in files that have all the tags", () => {
    const tags: Record<string, string[]> = {
      "docs/auth.md": ["spec", "auth"],
      "README.md": ["spec"],
    };
    const rows = sectionRows({
      ...base,
      tags: ["spec", "auth"],
      tagsOf: (p) => tags[p],
      hits: [hit({ path: "README.md" }), hit({ path: "docs/auth.md", startLine: 7 })],
    });
    expect(rows.map((r) => r.path)).toEqual(["docs/auth.md"]);
  });

  it("lists the files of the project before their copies in worktrees, each in the order given", () => {
    const rows = sectionRows({
      ...base,
      hits: [
        hit({ path: "@wt/feat/docs/auth.md", headingPath: ["Auth"], startLine: 1 }),
        hit({ path: "docs/auth.md", headingPath: ["Auth"], startLine: 2 }),
        hit({ path: "README.md", startLine: 3 }),
      ],
    });
    expect(rows.map((r) => r.path)).toEqual(["docs/auth.md", "README.md", "@wt/feat/docs/auth.md"]);
    expect(rows[2].where).toBe("auth.md @wt feat");
  });

  it("stops at the limit", () => {
    const hits = Array.from({ length: 10 }, (_, i) => hit({ path: "docs/auth.md", startLine: i }));
    expect(sectionRows({ ...base, hits, limit: 4 })).toHaveLength(4);
  });
});

describe("outlineRows", () => {
  it("lists the headings in order, each with the headings above it", () => {
    const rows = outlineRows(
      "docs/auth.md",
      [
        { index: 0, text: "Auth", parent: null },
        { index: 1, text: "Login", parent: 0 },
        { index: 2, text: "Tokens", parent: 1 },
        { index: 3, text: "Logout", parent: 0 },
      ],
      "Untitled",
    );
    expect(rows.map((r) => r.title)).toEqual(["Auth", "Login", "Tokens", "Logout"]);
    expect(rows.map((r) => r.where)).toEqual([
      "auth.md",
      "auth.md › Auth",
      "auth.md › Auth › Login",
      "auth.md › Auth",
    ]);
    expect(rows[2]).toMatchObject({ outline: 2, hit: null, snippet: null, key: "outline:2" });
  });

  it("names an empty heading", () => {
    const [row] = outlineRows("a.md", [{ index: 0, text: "", parent: null }], "Untitled");
    expect(row.title).toBe("Untitled");
  });
});

describe("tagRows", () => {
  const counts = [
    { name: "spec", count: 9 },
    { name: "auth", count: 12 },
    { name: "authoring", count: 2 },
    { name: "research", count: 4 },
  ];

  it("lists every tag by file count when nothing is typed", () => {
    expect(tagRows("", counts, []).map((r) => r.name)).toEqual([
      "auth",
      "spec",
      "research",
      "authoring",
    ]);
  });

  it("ranks the matching tags, better matches first", () => {
    const rows = tagRows("auth", counts, []);
    expect(rows.map((r) => r.name)).toEqual(["auth", "authoring"]);
    expect(rows[0]).toMatchObject({
      type: "tag",
      key: "tag:auth",
      count: 12,
      positions: [0, 1, 2, 3],
    });
  });

  it("leaves out the tags already chosen as filters", () => {
    expect(tagRows("", counts, ["auth", "spec"]).map((r) => r.name)).toEqual([
      "research",
      "authoring",
    ]);
  });

  it("finds nothing for a word no tag has", () => {
    expect(tagRows("zzz", counts, [])).toEqual([]);
  });
});

describe("commandRows", () => {
  it("writes each shortcut for the platform and keeps the label and the alias", () => {
    const matches: CommandMatch[] = [
      { command: COMMANDS[0], label: "Ordner öffnen", positions: [0], alias: null },
      {
        command: COMMANDS.find((c) => c.id === "checkUpdates") ?? COMMANDS[0],
        label: "U",
        positions: [],
        alias: "Check for updates",
      },
    ];
    const rows = commandRows(matches, (combo) => `<${combo}>`);
    expect(rows[0]).toMatchObject({
      type: "command",
      key: "command:openFolder",
      id: "openFolder",
      label: "Ordner öffnen",
      shortcut: "<Ctrl+O>",
      alias: null,
    });
    expect(rows[1]).toMatchObject({
      id: "checkUpdates",
      shortcut: null,
      alias: "Check for updates",
    });
  });
});

// ---- the view ------------------------------------------------------------------------------------------

const file = (path: string, over: Partial<FileRow> = {}): FileRow => ({
  type: "file",
  key: `file:${path}`,
  path,
  positions: [],
  descriptionPositions: [],
  description: "",
  ms: null,
  ...over,
});
const section = (n: number): SectionRow => ({
  type: "section",
  key: `section:${n}`,
  path: "a.md",
  title: `Section ${n}`,
  where: "a.md",
  snippet: "",
  hit: null,
  outline: n,
});
const tag = (name: string): TagRow => ({
  type: "tag",
  key: `tag:${name}`,
  name,
  count: 1,
  positions: [],
});
const command = (id: CommandRow["id"]): CommandRow => ({
  type: "command",
  key: `command:${id}`,
  id,
  label: id,
  positions: [],
  alias: null,
  shortcut: null,
});
const many = <T>(n: number, make: (i: number) => T): T[] =>
  Array.from({ length: n }, (_, i) => make(i));

function view(over: Partial<ViewInput>) {
  return buildView({
    scope: "all",
    idle: false,
    hasWords: true,
    files: [],
    moreHidden: 0,
    sections: [],
    tags: [],
    commands: [],
    ...over,
  });
}
const keysOf = (rows: Row[]): string[] => rows.map((r) => r.key);

describe("buildView: All", () => {
  it("groups Files, Sections, Tags and Commands in that fixed order, whatever the scores", () => {
    const v = view({
      files: [file("a.md")],
      sections: [section(1)],
      tags: [tag("auth")],
      commands: [command("save")],
    });
    expect(v.groups.map((g) => g.title)).toEqual(["files", "sections", "tags", "commands"]);
    expect(keysOf(v.rows)).toEqual(["file:a.md", "section:1", "tag:auth", "command:save"]);
  });

  it("limits the groups to 5, 5, 3 and 3 rows and points the rest at its scope", () => {
    const v = view({
      files: many(7, (i) => file(`f${i}.md`)),
      sections: many(23, section),
      tags: many(3, (i) => tag(`t${i}`)),
      commands: many(9, (i) => command(i % 2 ? "save" : "find")),
    });
    expect(GROUP_LIMIT).toEqual({ files: 5, sections: 5, tags: 3, commands: 3 });
    expect(v.groups.map((g) => g.rows.length)).toEqual([5, 5, 3, 3]);
    expect(v.groups.map((g) => g.total)).toEqual([7, 23, 3, 9]);
    expect(v.groups.map((g) => g.showAll)).toEqual(["files", "sections", null, "commands"]);
    expect(v.rows).toHaveLength(16);
  });

  it("leaves out a group with no results", () => {
    const v = view({ files: [file("a.md")], commands: [command("save")] });
    expect(v.groups.map((g) => g.title)).toEqual(["files", "commands"]);
    expect(view({}).groups).toEqual([]);
    expect(view({}).rows).toEqual([]);
  });

  it("puts the best file first: the first row is a file whenever one matches", () => {
    const v = view({ files: [file("a.md")], sections: [section(1)], commands: [command("save")] });
    expect(v.rows[0].type).toBe("file");
  });

  it("lists the recent files, under their own headers, when nothing is typed", () => {
    const v = view({
      idle: true,
      hasWords: false,
      files: [file("a.md", { group: "changed" }), file("b.md", { group: "opened" })],
      sections: [section(1)],
      tags: [tag("x")],
      commands: [command("save")],
    });
    expect(v.groups.map((g) => g.title)).toEqual(["changed", "opened"]);
    expect(keysOf(v.rows)).toEqual(["file:a.md", "file:b.md"]);
  });

  it("leaves out the empty list of recent files", () => {
    const v = view({ idle: true, hasWords: false, files: [file("b.md", { group: "opened" })] });
    expect(v.groups.map((g) => g.title)).toEqual(["opened"]);
  });

  it("lists only the files that have the tags when there are tags and no words", () => {
    const v = view({
      hasWords: false,
      files: [file("a.md")],
      sections: [section(1)],
      tags: [tag("x")],
      commands: [command("save")],
    });
    expect(v.groups.map((g) => g.title)).toEqual(["files"]);
  });

  it("does not offer the hidden files in All", () => {
    const v = view({ files: [file("a.md")], moreHidden: 4 });
    expect(v.rows.some((r) => r.type === "more")).toBe(false);
  });
});

describe("buildView: one scope", () => {
  it("Files: the matches under no header, then the row of hidden matches", () => {
    const v = view({ scope: "files", files: [file("a.md"), file("b.md")], moreHidden: 3 });
    expect(v.groups).toHaveLength(1);
    expect(v.groups[0]).toMatchObject({ title: null, total: 2, showAll: null });
    expect(keysOf(v.rows)).toEqual(["file:a.md", "file:b.md", "more"]);
    expect(v.rows[2]).toEqual({ type: "more", key: "more", count: 3 });
  });

  it("Files: no row of hidden matches when none are hidden", () => {
    const v = view({ scope: "files", files: [file("a.md")] });
    expect(keysOf(v.rows)).toEqual(["file:a.md"]);
  });

  it("Files: the recent files, as Quick open lists them, when nothing is typed", () => {
    const v = view({
      scope: "files",
      idle: true,
      hasWords: false,
      moreHidden: 5,
      files: [file("a.md", { group: "changed" }), file("b.md", { group: "opened" })],
    });
    expect(v.groups.map((g) => g.title)).toEqual(["changed", "opened"]);
    expect(v.rows.some((r) => r.type === "more")).toBe(false);
  });

  it("Sections, Tags and Commands list every result under no header", () => {
    const sections = many(12, section);
    expect(view({ scope: "sections", sections }).rows).toHaveLength(12);
    expect(view({ scope: "sections", sections }).groups[0]).toMatchObject({
      title: null,
      showAll: null,
    });
    expect(view({ scope: "tags", tags: many(8, (i) => tag(`t${i}`)) }).rows).toHaveLength(8);
    expect(
      view({ scope: "commands", commands: many(20, () => command("save")) }).rows,
    ).toHaveLength(20);
  });

  it("lists the scope's own rows even when nothing is typed (outline, every tag, every command)", () => {
    const v = view({ scope: "sections", idle: true, hasWords: false, sections: many(3, section) });
    expect(v.rows).toHaveLength(3);
    expect(
      view({ scope: "tags", idle: true, hasWords: false, tags: [tag("a")] }).rows,
    ).toHaveLength(1);
    expect(
      view({ scope: "commands", idle: true, hasWords: false, commands: [command("save")] }).rows,
    ).toHaveLength(1);
  });

  it("has no group when the scope has no results", () => {
    for (const scope of ["files", "sections", "tags", "commands"] as const) {
      expect(view({ scope }).groups, scope).toEqual([]);
      expect(view({ scope }).rows, scope).toEqual([]);
    }
  });

  it("ignores the lists of the other scopes", () => {
    const v = view({
      scope: "tags",
      files: [file("a.md")],
      sections: [section(1)],
      tags: [tag("x")],
    });
    expect(keysOf(v.rows)).toEqual(["tag:x"]);
  });
});

describe("moveActive", () => {
  it("moves down and up through the rows, wrapping around the ends", () => {
    expect(moveActive(0, 1, 3)).toBe(1);
    expect(moveActive(2, 1, 3)).toBe(0);
    expect(moveActive(0, -1, 3)).toBe(2);
    expect(moveActive(2, -1, 3)).toBe(1);
  });

  it("stays at the first row when there are none", () => {
    expect(moveActive(0, 1, 0)).toBe(0);
    expect(moveActive(0, -1, 0)).toBe(0);
  });

  it("follows the order of the groups", () => {
    const v = view({ files: [file("a.md"), file("b.md")], sections: [section(1)] });
    let active = 0;
    const walk: string[] = [v.rows[active].key];
    for (let i = 0; i < 3; i++) {
      active = moveActive(active, 1, v.rows.length);
      walk.push(v.rows[active].key);
    }
    expect(walk).toEqual(["file:a.md", "file:b.md", "section:1", "file:a.md"]);
  });
});

import { describe, expect, it } from "vitest";
import type { ProjectRow, ProjectsView } from "../../api";
import {
  aliasOf,
  depthOf,
  folderName,
  newWhere,
  groupsOf,
  hueOfMember,
  joinNames,
  keyOf,
  membersOf,
  middleCut,
  offeredPaths,
  ownerOf,
  recordPlace,
  relativeTo,
  stripOf,
} from "./model";
import { hueOf } from "../shell/switcherModel";

function row(path: string, fields: Partial<ProjectRow> = {}): ProjectRow {
  const name = path.split("/").pop() ?? path;
  return {
    key: path.toLowerCase(),
    uid: null,
    path,
    state: "offered",
    missing: null,
    marker: "git",
    name,
    short: name,
    description: null,
    category: null,
    hidden: "own",
    hiddenUnknown: false,
    agent: "same",
    parent: null,
    inner: null,
    cardPlace: null,
    locateFrom: null,
    ...fields,
  };
}

function view(fields: Partial<ProjectsView> = {}): ProjectsView {
  return {
    seq: 1,
    present: true,
    ready: true,
    answer: "unanswered",
    rows: [],
    offered: { count: 0, names: [], inner: [] },
    record: { status: "absent" },
    skipped: [],
    ask: null,
    rootCard: {
      name: "root",
      short: "root",
      description: null,
      category: null,
      place: "profile",
      invalid: [],
      warning: null,
    },
    ...fields,
  };
}

const offered = (count: number) => ({
  count,
  names: ["a", "b", "c"].slice(0, count),
  inner: [],
});

describe("strip", () => {
  it("asks the first question, then about new projects, then nothing", () => {
    expect(stripOf(view({ offered: offered(2) })).kind).toBe("first");
    expect(stripOf(view({ answer: "answered", offered: offered(1) })).kind).toBe("new");
    expect(stripOf(view({ answer: "declinedAll", offered: offered(3) })).kind).toBe("none");
    expect(stripOf(view({ answer: "answered" })).kind).toBe("none");
  });

  it("puts the move question first", () => {
    const ask = { uid: "abcdefghij", name: "hub-x", was: "hubs/hub-x", candidate: "hubs/b" };
    const strip = stripOf(view({ answer: "answered", offered: offered(2), ask }));
    expect(strip).toEqual({ kind: "move", ask });
  });

  // adversarial: nothing before the walk ends, in a plain folder, or while the list is newer
  it("stays quiet while loading, in a plain folder and while the list is newer", () => {
    expect(stripOf(null).kind).toBe("none");
    expect(stripOf(view({ ready: false, offered: offered(2) })).kind).toBe("none");
    expect(stripOf(view({ present: false, offered: offered(2) })).kind).toBe("none");
    expect(stripOf(view({ record: { status: "newer" }, offered: offered(2) })).kind).toBe("none");
  });

  it("an unreadable list says so, before anything else (also while the walk runs) (F7)", () => {
    const unreadable = { status: "unreadable" as const, line: 3, message: "x" };
    expect(stripOf(view({ record: unreadable, offered: offered(2) })).kind).toBe("broken");
    expect(stripOf(view({ record: unreadable, ready: false })).kind).toBe("broken");
  });

  it("new projects carry their paths (where they are) (F8)", () => {
    const v = view({
      answer: "answered",
      offered: offered(2),
      rows: [row("b/two"), row("a/one"), row("c/kept", { state: "included" })],
    });
    expect(stripOf(v)).toMatchObject({ kind: "new", paths: ["a/one", "b/two"] });
  });
});

describe("where new projects are (F8)", () => {
  it("one | up to 3 paths | a count in the folder holding most and how many others", () => {
    expect(newWhere(["tools/new-app"], 1)).toEqual({ kind: "one", path: "tools/new-app" });
    expect(newWhere(["a/x", "b/y"], 2)).toEqual({ kind: "few", paths: ["a/x", "b/y"] });
    expect(newWhere(["a/x", "incoming/1", "incoming/2", "z/q", "incoming/3"], 5)).toEqual({
      kind: "many",
      count: 5,
      folder: "incoming",
      others: 2,
    });
    expect(newWhere(["incoming/1", "incoming/2", "incoming/3", "incoming/4"], 4)).toEqual({
      kind: "many",
      count: 4,
      folder: "incoming",
      others: 0,
    });
  });

  // adversarial: projects at the top of the folder; a tie; fewer paths than the count says
  it("top-level projects, ties and a short list", () => {
    expect(newWhere(["a", "b", "c", "d"], 4)).toEqual({
      kind: "many",
      count: 4,
      folder: "",
      others: 0,
    });
    expect(newWhere(["x/1", "y/1", "x/2", "y/2"], 4)).toMatchObject({ folder: "x", others: 1 });
    expect(newWhere(["x/1"], 3)).toEqual({ kind: "many", count: 3, folder: "x", others: 0 });
  });
});

describe("project node label (F1)", () => {
  it("the short name only when it says something else than the folder name", () => {
    expect(aliasOf({ path: "hubs/hub-b", short: "hub-b" })).toBeNull();
    expect(aliasOf({ path: "hubs/hub-b", short: "Hub-B" })).toBeNull();
    expect(aliasOf({ path: "hubs/hub-b", short: "PerMark" })).toBe("PerMark");
    expect(aliasOf({ path: "repos/Café NFD", short: "Café NFD" })).toBeNull();
    expect(aliasOf({ path: "x/y", short: "  " })).toBeNull();
    expect(folderName("a/b/c")).toBe("c");
  });
});

describe("owner of a path", () => {
  const rows = [
    row("hubs/hub-b", { state: "included" }),
    row("hubs/hub-b/projects/spec", { state: "included" }),
    row("hubs/hub-bc", { state: "offered" }),
    row("Tools/Notes", { state: "included" }),
  ];
  const members = membersOf(view({ rows }));

  it("is the deepest included project containing it, the folder itself included", () => {
    expect(ownerOf(members, "hubs/hub-b/a.md")?.path).toBe("hubs/hub-b");
    expect(ownerOf(members, "hubs/hub-b/projects/spec/plan.md")?.path).toBe(
      "hubs/hub-b/projects/spec",
    );
    expect(ownerOf(members, "hubs/hub-b")?.path).toBe("hubs/hub-b");
  });

  // adversarial: a sibling with the same start, a not-included project, case, separators, worktrees, the root
  it("never matches a sibling prefix, a not-included project or a worktree", () => {
    expect(ownerOf(members, "hubs/hub-bc/a.md")).toBeNull();
    expect(ownerOf(members, "hubs/a.md")).toBeNull();
    expect(ownerOf(members, "README.md")).toBeNull();
    expect(ownerOf(members, "@wt/feature/hubs/hub-b/a.md")).toBeNull();
    expect(ownerOf(members, null)).toBeNull();
    expect(ownerOf(members, "tools/notes/x.md")?.path).toBe("Tools/Notes");
    expect(ownerOf(members, "hubs\\hub-b\\a.md")?.path).toBe("hubs/hub-b");
  });

  it("matches NFC and NFD spellings of one name", () => {
    const nfd = "Cafe\u0301";
    const m = membersOf(view({ rows: [row(nfd, { state: "included" })] }));
    expect(ownerOf(m, "Café/a.md")?.path).toBe(nfd);
    expect(keyOf("Café")).toBe(keyOf(nfd));
  });

  it("gives the path inside the project", () => {
    const hub = rows[0]!;
    expect(relativeTo(hub, "hubs/hub-b/docs/a.md")).toBe("docs/a.md");
    expect(relativeTo(hub, "hubs/hub-b")).toBe("");
  });
});

describe("Settings groups", () => {
  const rows = [
    row("b", { state: "included" }),
    row("a", { state: "declined" }),
    row("c", { state: "offered" }),
    row("x", { state: "missing", missing: "gone" }),
    row("b/inner", { state: "included", parent: "b" }),
    row("b/inner/deep", { state: "offered", parent: "b/inner" }),
  ];

  it("groups by state in path order and filters by name | short | path", () => {
    const g = groupsOf(rows, "");
    expect(g.included.map((r) => r.path)).toEqual(["b", "b/inner"]);
    expect(g.notIncluded.map((r) => r.path)).toEqual(["a", "b/inner/deep", "c"]);
    expect(g.notFound.map((r) => r.path)).toEqual(["x"]);
    expect(groupsOf(rows, "INNER").included.map((r) => r.path)).toEqual(["b/inner"]);
    expect(groupsOf(rows, "  b   deep ").notIncluded.map((r) => r.path)).toEqual(["b/inner/deep"]);
    expect(groupsOf(rows, "nothing-like-this").included).toEqual([]);
  });

  it("indents by the rows of its group that contain it; a broken | looping chain ends", () => {
    const byKey = new Map(rows.map((r) => [r.key, r]));
    expect(depthOf(rows[4]!, byKey)).toBe(1);
    expect(depthOf(rows[5]!, byKey)).toBe(0);
    const loop = [row("p", { parent: "q" }), row("q", { parent: "p" })];
    const loopKeys = new Map(loop.map((r) => [r.key, r]));
    expect(depthOf(loop[0]!, loopKeys)).toBeLessThanOrEqual(2);
    expect(depthOf(row("z", { parent: "missing" }), byKey)).toBe(0);
  });

  it("lists the offered ones for Choose, in path order", () => {
    expect(offeredPaths(view({ rows }))).toEqual(["b/inner/deep", "c"]);
    expect(offeredPaths(null)).toEqual([]);
  });
});

describe("words and colors", () => {
  it("joins names the way the language does", () => {
    expect(joinNames(["a", "b", "c"], "en")).toBe("a, b, and c");
    expect(joinNames(["a"], "en")).toBe("a");
    expect(joinNames(["a", "b"], "de")).toBe("a und b");
  });

  it("cuts long paths in the middle, keeping the last folder", () => {
    expect(middleCut("short/path")).toBe("short/path");
    const long = `${"x".repeat(60)}/last`;
    const cut = middleCut(long);
    expect(Array.from(cut).length).toBeLessThanOrEqual(48);
    expect(cut.endsWith("…/last")).toBe(true);
    expect(middleCut(`a/${"y".repeat(90)}`).startsWith("…")).toBe(true);
    // Unicode counted by characters, not UTF-16 units
    expect(Array.from(middleCut(`${"数".repeat(60)}/据`)).length).toBeLessThanOrEqual(48);
  });

  it("gives a project the hue its own tab would wear", () => {
    const r = row("hubs/hub-b");
    expect(hueOfMember("C:\\Users\\me\\root\\", r)).toBe(
      hueOf({ kind: "project", path: "C:/Users/me/root/hubs/hub-b" }),
    );
  });
});

describe("recordPlace: where an answer is saved, said before it is given (S2-1)", () => {
  it("no list yet -> PerMark creates it; a list (also broken | newer) -> PerMark saves in it", () => {
    expect(recordPlace(null)).toBe("projects.record.creates");
    expect(recordPlace(view({ record: { status: "absent" } }))).toBe("projects.record.creates");
    expect(recordPlace(view({ record: { status: "ok" } }))).toBe("projects.record.saves");
    expect(recordPlace(view({ record: { status: "newer" } }))).toBe("projects.record.saves");
    expect(recordPlace(view({ record: { status: "unreadable", line: 2, message: "x" } }))).toBe(
      "projects.record.saves",
    );
  });
});

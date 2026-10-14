import { describe, expect, it } from "vitest";
import {
  DEFAULT_TREE_FILTER,
  categoriesOf,
  documentFolders,
  fileTest,
  filterFromDefault,
  filterTree,
  normalizeTreeFilter,
  parsePatterns,
  splitPatterns,
  withShow,
  type FileTest,
} from "./treeFilter";
import type { TreeFilter, TreeNode } from "./types";

const file = (path: string): TreeNode => ({
  name: path.split("/").pop() ?? path,
  path,
  kind: "file",
});
const dir = (path: string, children: TreeNode[]): TreeNode => ({
  name: path.split("/").pop() ?? path,
  path,
  kind: "dir",
  children,
});
const worktree = (name: string, children: TreeNode[]): TreeNode => ({
  name,
  path: `@wt/${name}`,
  kind: "worktree",
  children,
});

// these tests are about categories and patterns; "assets next to documents" has its own section below
const custom = (patch: Partial<TreeFilter>): TreeFilter => ({
  show: "custom",
  categories: [],
  patterns: "",
  hideEmptyFolders: true,
  assetsNearDocs: false,
  ...patch,
});
const ROOT_DOCS: ReadonlySet<string> = new Set([""]);
const shownOf = (filter: TreeFilter, paths: string[]): string[] => {
  const test = fileTest(filter, documentFolders(paths));
  return paths.filter(test);
};

describe("presets", () => {
  const paths = [
    "README.md",
    "docs/guide.markdown",
    "docs/page.mdx",
    "notes.txt",
    "logo.png",
    "package.json",
    "ci.yaml",
    "Cargo.toml",
    "feed.xml",
    "rows.csv",
    "src/main.rs",
    "src/app.ts",
    "Cargo.lock",
    "Dockerfile",
  ];

  it("Documents shows .md .markdown .mdx and .txt", () => {
    expect(shownOf({ ...DEFAULT_TREE_FILTER, show: "documents" }, paths)).toEqual([
      "README.md",
      "docs/guide.markdown",
      "docs/page.mdx",
      "notes.txt",
    ]);
  });

  it("Docs and assets adds images, JSON, YAML, TOML, XML and CSV", () => {
    expect(shownOf(DEFAULT_TREE_FILTER, paths)).toEqual([
      "README.md",
      "docs/guide.markdown",
      "docs/page.mdx",
      "notes.txt",
      "logo.png",
      "package.json",
      "ci.yaml",
      "Cargo.toml",
      "feed.xml",
      "rows.csv",
    ]);
  });

  it("Everything shows every file", () => {
    expect(shownOf({ ...DEFAULT_TREE_FILTER, show: "everything" }, paths)).toEqual(paths);
  });

  it("the default is Docs and assets with empty folders hidden", () => {
    expect(DEFAULT_TREE_FILTER.show).toBe("docs-and-assets");
    expect(DEFAULT_TREE_FILTER.hideEmptyFolders).toBe(true);
    expect(DEFAULT_TREE_FILTER.assetsNearDocs).toBe(true);
  });

  it("Custom shows the checked categories", () => {
    const filter = custom({ categories: ["images", "other"] });
    expect(shownOf(filter, paths)).toEqual([
      "logo.png",
      "src/main.rs",
      "src/app.ts",
      "Cargo.lock",
      "Dockerfile",
    ]);
    expect(shownOf(custom({}), paths)).toEqual([]);
  });

  it("the presets ignore the patterns and categories of a previous Custom choice", () => {
    const filter: TreeFilter = {
      ...DEFAULT_TREE_FILTER,
      show: "documents",
      categories: ["other"],
      patterns: "+*.rs",
    };
    expect(shownOf(filter, paths)).not.toContain("src/main.rs");
  });
});

describe("patterns", () => {
  const paths = [
    "queries/users.sql",
    "db/schema.sql",
    "docs/a.md",
    "docs/archive/old.md",
    "archive/old.md",
    "src/archive.md",
    "drafts/x.md",
    "docs/Notes.MD",
  ];
  const docs = ["documents"] as const;
  const run = (patterns: string, categories: readonly "documents"[] = docs): string[] =>
    shownOf(custom({ categories: [...categories], patterns }), paths);

  it("+pattern adds files and -pattern removes them", () => {
    expect(run("+*.sql", [])).toEqual(["queries/users.sql", "db/schema.sql"]);
    expect(run("-archive/**")).not.toContain("archive/old.md");
  });

  it("a name without a slash matches at any depth, with a slash it is anchored at the root", () => {
    expect(run("-archive/")).toEqual([
      "docs/a.md",
      "src/archive.md",
      "drafts/x.md",
      "docs/Notes.MD",
    ]);
    expect(run("-archive/**")).toEqual([
      "docs/a.md",
      "docs/archive/old.md",
      "src/archive.md",
      "drafts/x.md",
      "docs/Notes.MD",
    ]);
    expect(run("-/archive")).not.toContain("archive/old.md");
    expect(run("-/archive")).toContain("docs/archive/old.md");
  });

  it("a pattern that names a folder covers what is inside it", () => {
    expect(run("-docs/archive")).not.toContain("docs/archive/old.md");
    expect(run("-docs")).toEqual(["archive/old.md", "src/archive.md", "drafts/x.md"]);
  });

  it("the last matching pattern wins", () => {
    expect(run("-docs/** +docs/a.md")).toEqual([
      "docs/a.md",
      "archive/old.md",
      "src/archive.md",
      "drafts/x.md",
    ]);
    expect(run("+docs/a.md -docs/**")).not.toContain("docs/a.md");
  });

  it("a bare pattern shows, like +pattern", () => {
    expect(run("*.sql", [])).toEqual(run("+*.sql", []));
  });

  it("matches ignoring case and understands ?, [..] and {a,b}", () => {
    expect(run("+*.md", [])).toContain("docs/Notes.MD");
    expect(run("+docs/?.md", [])).toEqual(["docs/a.md"]);
    expect(run("+[dq]*/*.sql", [])).toEqual(["queries/users.sql", "db/schema.sql"]);
    expect(run("+*.{sql,md}", [])).toContain("db/schema.sql");
    expect(run("+*.{sql,md}", [])).toContain("docs/a.md");
    expect(run("+**/schema.*", [])).toEqual(["db/schema.sql"]);
  });

  it("separates entries with spaces, commas and new lines", () => {
    expect(splitPatterns("+*.sql, -archive/**\n+a/b")).toEqual(["+*.sql", "-archive/**", "+a/b"]);
    expect(splitPatterns("+*.{sql,md},-x")).toEqual(["+*.{sql,md}", "-x"]);
    expect(splitPatterns("  ")).toEqual([]);
  });

  it("reports malformed entries and keeps working with the valid ones", () => {
    const parsed = parsePatterns("+*.sql +[abc -{a,b + - +x/{ +[z-a]");
    expect(parsed.rules.map((r) => r.source)).toEqual(["+*.sql"]);
    expect(parsed.invalid).toEqual(["+[abc", "-{a,b", "+", "-", "+x/{", "+[z-a]"]);
    const filter = custom({ patterns: "+*.sql +[abc" });
    expect(shownOf(filter, paths)).toEqual(["queries/users.sql", "db/schema.sql"]);
  });

  it("applies to worktree files relative to the worktree root", () => {
    const filter = custom({ categories: ["documents"], patterns: "-docs/archive/**" });
    expect(fileTest(filter, ROOT_DOCS)("@wt/feature/docs/archive/old.md")).toBe(false);
    expect(fileTest(filter, ROOT_DOCS)("@wt/feature/docs/a.md")).toBe(true);
    expect(fileTest(custom({ patterns: "+/queries/*.sql" }), ROOT_DOCS)("@wt/f/queries/u.sql")).toBe(true);
  });
});

describe("the project's default", () => {
  it("is Docs and assets when the project says nothing", () => {
    expect(filterFromDefault(null)).toEqual(DEFAULT_TREE_FILTER);
    expect(filterFromDefault({ show: null, include: [], exclude: [] })).toEqual(
      DEFAULT_TREE_FILTER,
    );
  });

  it("picks the preset it names", () => {
    const f = filterFromDefault({ show: "documents", include: [], exclude: [] });
    expect(f.show).toBe("documents");
    expect(f.patterns).toBe("");
  });

  it("turns patterns into a Custom filter that starts from the preset", () => {
    const f = filterFromDefault({
      show: "documents",
      include: ["*.sql"],
      exclude: ["archive/**", "*.tmp"],
    });
    expect(f.show).toBe("custom");
    expect(f.categories).toEqual(categoriesOf("documents"));
    expect(f.patterns).toBe("+*.sql -archive/** -*.tmp");
    const test = fileTest(f, ROOT_DOCS);
    expect(test("a.md")).toBe(true);
    expect(test("db/q.sql")).toBe(true);
    expect(test("archive/old.md")).toBe(false);
    expect(test("logo.png")).toBe(false);
  });

  it("with no preset, include and exclude adjust the app's default", () => {
    const f = filterFromDefault({ show: null, include: [], exclude: ["archive/**"] });
    const test = fileTest(f, ROOT_DOCS);
    expect(test("logo.png")).toBe(true);
    expect(test("archive/old.md")).toBe(false);
  });

  it("show = custom starts from nothing: only what `include` lists", () => {
    const f = filterFromDefault({ show: "custom", include: ["docs/**"], exclude: [] });
    const test = fileTest(f, ROOT_DOCS);
    expect(test("docs/a.md")).toBe(true);
    expect(test("README.md")).toBe(false);
  });

  it("Everything with excludes keeps every category checked", () => {
    const f = filterFromDefault({ show: "everything", include: [], exclude: ["dist/"] });
    expect(fileTest(f, ROOT_DOCS)("src/main.rs")).toBe(true);
    expect(fileTest(f, ROOT_DOCS)("dist/app.js")).toBe(false);
  });
});

describe("withShow / normalizeTreeFilter", () => {
  it("Custom starts from what the previous preset showed", () => {
    const f = withShow(DEFAULT_TREE_FILTER, "custom");
    expect(f.show).toBe("custom");
    expect(f.categories).toEqual(["documents", "text", "images", "data", "xml"]);
    // going back and forth inside Custom keeps the person's boxes
    const edited = { ...f, categories: ["documents" as const] };
    expect(withShow(edited, "custom")).toBe(edited);
    expect(withShow(withShow(edited, "everything"), "custom").categories).toEqual(
      categoriesOf("everything"),
    );
  });

  it("keeps a valid stored filter and repairs the rest", () => {
    const stored = custom({ categories: ["data"], patterns: "+a", hideEmptyFolders: false });
    expect(normalizeTreeFilter(stored)).toEqual(stored);
    expect(normalizeTreeFilter({ show: "everything" })).toEqual({
      show: "everything",
      categories: [],
      patterns: "",
      hideEmptyFolders: true,
      assetsNearDocs: true,
    });
    expect(
      normalizeTreeFilter({ show: "custom", categories: ["data", "nope", "data", 3], patterns: 4 }),
    ).toEqual(custom({ categories: ["data"], assetsNearDocs: true }));
  });

  it("assets next to documents is on unless a stored filter turned it off", () => {
    expect(normalizeTreeFilter({ show: "documents" })?.assetsNearDocs).toBe(true);
    expect(normalizeTreeFilter({ show: "documents", assetsNearDocs: "no" })?.assetsNearDocs).toBe(
      true,
    );
    expect(normalizeTreeFilter({ show: "custom", assetsNearDocs: false })?.assetsNearDocs).toBe(
      false,
    );
  });

  it("ignores a stored filter that names no known preset", () => {
    expect(normalizeTreeFilter(null)).toBeNull();
    expect(normalizeTreeFilter(undefined)).toBeNull();
    expect(normalizeTreeFilter("documents")).toBeNull();
    expect(normalizeTreeFilter({ show: "all-the-things" })).toBeNull();
    expect(normalizeTreeFilter({})).toBeNull();
  });
});

// This repository's own layout (trimmed): the example that shaped the rule. Code folders hold JSON, TOML,
// YAML and PNG files that are not companions of any document.
const REPO = [
  ".claude/agents/rust-reviewer.md",
  ".claude/agents/ux-designer.md",
  ".claude/hooks/format-on-edit.mjs",
  ".claude/settings.json",
  ".claude/skills/design-system/SKILL.md",
  ".github/workflows/ci.yml",
  ".github/workflows/pages.yml",
  ".github/workflows/release.yml",
  ".gitignore",
  ".prettierrc",
  ".vscode/extensions.json",
  "CHANGELOG.md",
  "CLAUDE.md",
  "HANDOFF.md",
  "LICENSE-APACHE",
  "README.md",
  "docs/assets/logo.png",
  "docs/assets/logo.svg",
  "docs/assets/screenshot-dark.png",
  "docs/design/mockup.html",
  "docs/design/v0.2.md",
  "docs/design/wireframes.md",
  "docs/plan.md",
  "docs/requirements.md",
  "index.html",
  "package-lock.json",
  "package.json",
  "site/assets/logo.svg",
  "site/assets/screenshot-dark.png",
  "site/assets/screenshot-light.png",
  "site/index.html",
  "site/main.js",
  "site/styles.css",
  "src-tauri/Cargo.lock",
  "src-tauri/Cargo.toml",
  "src-tauri/build.rs",
  "src-tauri/capabilities/default.json",
  "src-tauri/icons/32x32.png",
  "src-tauri/icons/128x128.png",
  "src-tauri/icons/icon.ico",
  "src-tauri/icons/icon.png",
  "src-tauri/ipc-contract.json",
  "src-tauri/src/lib.rs",
  "src-tauri/src/main.rs",
  "src-tauri/tauri.conf.json",
  "src/App.svelte",
  "src/lib/i18n/en.ts",
  "src/main.ts",
  "src/styles/tokens.css",
  "tsconfig.json",
  "vite.config.ts",
];

/** A tree with a file node for each path (folders are made on the way). */
function treeOf(paths: string[]): TreeNode {
  const root = dir("", []);
  for (const path of paths) {
    let node = root;
    const parts = path.split("/");
    parts.forEach((part, i) => {
      const here = parts.slice(0, i + 1).join("/");
      const children = node.children ?? [];
      let child = children.find((c) => c.path === here);
      if (!child) {
        child = i === parts.length - 1 ? file(here) : dir(here, []);
        children.push(child);
      }
      node = child;
    });
  }
  return root;
}

describe("assets next to documents", () => {
  const REPO_DOCS = [
    ".claude/agents/rust-reviewer.md",
    ".claude/agents/ux-designer.md",
    ".claude/skills/design-system/SKILL.md",
    "CHANGELOG.md",
    "CLAUDE.md",
    "HANDOFF.md",
    "README.md",
    "docs/design/v0.2.md",
    "docs/design/wireframes.md",
    "docs/plan.md",
    "docs/requirements.md",
  ];
  const REPO_ASSETS = [
    "docs/assets/logo.png",
    "docs/assets/logo.svg",
    "docs/assets/screenshot-dark.png",
    "package-lock.json",
    "package.json",
    "tsconfig.json",
  ];
  const testOf = (filter: TreeFilter, paths = REPO) => fileTest(filter, documentFolders(paths));

  it("keeps the code folders of this repository out of Docs and assets", () => {
    const shown = REPO.filter(testOf(DEFAULT_TREE_FILTER));
    expect([...shown].sort()).toEqual([...REPO_DOCS, ...REPO_ASSETS].sort());
    for (const hidden of [
      "src-tauri/Cargo.toml",
      "src-tauri/tauri.conf.json",
      "src-tauri/ipc-contract.json",
      "src-tauri/capabilities/default.json",
      "src-tauri/icons/icon.png",
      "src-tauri/icons/128x128.png",
      ".github/workflows/ci.yml",
      ".github/workflows/release.yml",
      ".vscode/extensions.json",
      ".claude/settings.json",
      "site/assets/logo.svg",
      "site/assets/screenshot-dark.png",
      "src/styles/tokens.css",
    ]) {
      expect(shown, hidden).not.toContain(hidden);
    }
  });

  it("shows assets next to documents: the same folder, or the folder above", () => {
    const test = testOf(DEFAULT_TREE_FILTER);
    expect(test("docs/assets/logo.png")).toBe(true); // docs/ holds documents, one level up
    expect(test("package.json")).toBe(true); // the root holds README.md
    expect(test("docs/design/mockup.html")).toBe(false); // other files are not assets either way
    const deeper = [...REPO, "docs/design/img/a.png", "docs/design/img/raw/b.png"];
    expect(testOf(DEFAULT_TREE_FILTER, deeper)("docs/design/img/a.png")).toBe(true);
    // two levels below the nearest document: too far
    expect(testOf(DEFAULT_TREE_FILTER, deeper)("docs/design/img/raw/b.png")).toBe(false);
  });

  it("does not borrow the root's documents for a first-level folder", () => {
    const test = testOf(DEFAULT_TREE_FILTER);
    expect(test("src-tauri/Cargo.toml")).toBe(false);
    expect(test("site/assets/logo.svg")).toBe(false); // site/ has no document of its own
  });

  it("reduces this repository's tree to its documents and their companions", () => {
    const outcome = filterTree(treeOf(REPO), testOf(DEFAULT_TREE_FILTER), {
      hideEmptyFolders: true,
      keep: null,
    });
    expect(REPO).toHaveLength(51);
    expect(outcome.shown).toHaveLength(17);
    expect(outcome.hidden).toHaveLength(34);
    expect(outcome.main.map((n) => n.path)).toEqual([
      ".claude",
      "CHANGELOG.md",
      "CLAUDE.md",
      "HANDOFF.md",
      "README.md",
      "docs",
      "package-lock.json",
      "package.json",
      "tsconfig.json",
    ]);
    const docs = outcome.main.find((n) => n.path === "docs");
    expect(docs?.children?.map((n) => n.path)).toEqual([
      "docs/assets",
      "docs/design",
      "docs/plan.md",
      "docs/requirements.md",
    ]);
  });

  it("turning it off brings back every image, JSON, YAML, TOML and XML file", () => {
    const shown = REPO.filter(testOf({ ...DEFAULT_TREE_FILTER, assetsNearDocs: false }));
    expect(shown).toContain("src-tauri/Cargo.toml");
    expect(shown).toContain("src-tauri/icons/icon.png");
    expect(shown).toContain(".github/workflows/ci.yml");
    expect(shown).toContain("site/assets/logo.svg");
    expect(shown).not.toContain("src-tauri/src/main.rs"); // still not a document or an asset
  });

  it("does not apply to Documents or Everything", () => {
    for (const assetsNearDocs of [true, false]) {
      const everything = { ...DEFAULT_TREE_FILTER, show: "everything" as const, assetsNearDocs };
      expect(REPO.filter(testOf(everything))).toEqual(REPO);
      const documents = { ...DEFAULT_TREE_FILTER, show: "documents" as const, assetsNearDocs };
      expect([...REPO.filter(testOf(documents))].sort()).toEqual([...REPO_DOCS].sort());
    }
  });

  it("applies to the asset categories of Custom, and only to those", () => {
    const filter: TreeFilter = {
      ...DEFAULT_TREE_FILTER,
      show: "custom",
      categories: ["images", "other"],
    };
    const test = testOf(filter);
    expect(test("docs/assets/logo.png")).toBe(true);
    expect(test("src-tauri/icons/icon.png")).toBe(false);
    expect(test("src/main.ts")).toBe(true); // "other" is not an asset category
    expect(test("package.json")).toBe(false); // data is not checked
    expect(testOf({ ...filter, assetsNearDocs: false })("src-tauri/icons/icon.png")).toBe(true);
  });

  it("explicit patterns still win, the last match first", () => {
    const filter: TreeFilter = {
      ...DEFAULT_TREE_FILTER,
      show: "custom",
      categories: categoriesOf("docs-and-assets"),
      patterns: "+src-tauri/*.json -package.json +site/assets/** -site/assets/logo.svg",
    };
    const test = testOf(filter);
    expect(test("src-tauri/tauri.conf.json")).toBe(true);
    expect(test("src-tauri/Cargo.toml")).toBe(false); // not matched by a pattern
    expect(test("package.json")).toBe(false);
    expect(test("site/assets/screenshot-dark.png")).toBe(true);
    expect(test("site/assets/logo.svg")).toBe(false);
  });

  it("works inside a worktree relative to the worktree's own folders", () => {
    const paths = [
      "package.json",
      "@wt/feature/README.md",
      "@wt/feature/package.json",
      "@wt/feature/docs/a.md",
      "@wt/feature/docs/assets/x.png",
      "@wt/feature/src-tauri/Cargo.toml",
      "@wt/feature/tools/b.png",
    ];
    const test = testOf(DEFAULT_TREE_FILTER, paths);
    expect(test("@wt/feature/package.json")).toBe(true); // the worktree root holds README.md
    expect(test("@wt/feature/docs/assets/x.png")).toBe(true);
    expect(test("@wt/feature/src-tauri/Cargo.toml")).toBe(false); // not the worktree root's documents
    expect(test("@wt/feature/tools/b.png")).toBe(false);
    // a worktree's README does not make the main folder's package.json an asset
    expect(test("package.json")).toBe(false);
  });

  it("finds the folders that hold documents", () => {
    expect([...documentFolders(REPO)].sort()).toEqual(
      ["", ".claude/agents", ".claude/skills/design-system", "docs", "docs/design"].sort(),
    );
    expect(documentFolders(["@wt/a/notes.txt", "x.png"])).toEqual(new Set(["@wt/a"]));
  });
});

describe("filterTree", () => {
  const root: TreeNode = dir("", [
    dir("docs", [
      dir("docs/specs", [file("docs/specs/live-feed.md"), file("docs/specs/model.rs")]),
      file("docs/index.md"),
    ]),
    dir("src", [dir("src/deep", [file("src/deep/a.rs")]), file("src/main.rs")]),
    dir("empty", []),
    file("README.md"),
    file("Cargo.lock"),
    worktree("feature-ui", [file("@wt/feature-ui/a.md"), file("@wt/feature-ui/b.rs")]),
    worktree("code-only", [dir("@wt/code-only/src", [file("@wt/code-only/src/x.rs")])]),
  ]);
  const docsOnly: FileTest = (p) => p.endsWith(".md");
  const names = (nodes: TreeNode[]): string[] => nodes.map((n) => n.path);

  it("keeps only the visible files and the folders that lead to them", () => {
    const out = filterTree(root, docsOnly, { hideEmptyFolders: true, keep: null });
    expect(names(out.main)).toEqual(["docs", "README.md"]);
    expect(names(out.main[0].children ?? [])).toEqual(["docs/specs", "docs/index.md"]);
    expect(names(out.main[0].children?.[0].children ?? [])).toEqual(["docs/specs/live-feed.md"]);
  });

  it("drops worktree roots with nothing to show, like any folder", () => {
    const out = filterTree(root, docsOnly, { hideEmptyFolders: true, keep: null });
    expect(names(out.worktrees)).toEqual(["@wt/feature-ui"]);
    expect(names(out.worktrees[0].children ?? [])).toEqual(["@wt/feature-ui/a.md"]);
    expect(out.worktrees[0].kind).toBe("worktree");
  });

  it("keeps the folders when asked to, even the empty ones", () => {
    const out = filterTree(root, docsOnly, { hideEmptyFolders: false, keep: null });
    expect(names(out.main)).toEqual(["docs", "src", "empty", "README.md"]);
    expect(out.main[1].children?.[0].children).toEqual([]);
    expect(names(out.worktrees)).toEqual(["@wt/feature-ui", "@wt/code-only"]);
  });

  it("counts the files that pass and the files that do not, worktrees included", () => {
    const out = filterTree(root, docsOnly, { hideEmptyFolders: true, keep: null });
    expect(out.shown).toEqual([
      "docs/specs/live-feed.md",
      "docs/index.md",
      "README.md",
      "@wt/feature-ui/a.md",
    ]);
    expect(out.hidden).toHaveLength(6);
    expect(out.shown.length + out.hidden.length).toBe(10);
  });

  it("keeps a hidden file in the tree while it is open, with the folders above it", () => {
    const out = filterTree(root, docsOnly, { hideEmptyFolders: true, keep: "src/deep/a.rs" });
    expect(names(out.main)).toEqual(["docs", "src", "README.md"]);
    expect(names(out.main[1].children ?? [])).toEqual(["src/deep"]);
    expect([...out.forced]).toEqual(["src/deep/a.rs"]);
    // it is still counted as hidden by the filter
    expect(out.hidden).toContain("src/deep/a.rs");
    expect(out.shown).not.toContain("src/deep/a.rs");
  });

  it("keeps an open hidden file of a worktree and its root", () => {
    const out = filterTree(root, docsOnly, {
      hideEmptyFolders: true,
      keep: "@wt/code-only/src/x.rs",
    });
    expect(names(out.worktrees)).toEqual(["@wt/feature-ui", "@wt/code-only"]);
    expect([...out.forced]).toEqual(["@wt/code-only/src/x.rs"]);
  });

  it("does not flag an open file that passes the filter, nor a path that is not in the tree", () => {
    expect(
      filterTree(root, docsOnly, { hideEmptyFolders: true, keep: "README.md" }).forced.size,
    ).toBe(0);
    expect(
      filterTree(root, docsOnly, { hideEmptyFolders: true, keep: "gone.rs" }).forced.size,
    ).toBe(0);
  });

  it("shows everything when everything passes (no folder is lost, empty ones aside)", () => {
    const out = filterTree(root, () => true, { hideEmptyFolders: true, keep: null });
    expect(out.hidden).toEqual([]);
    expect(names(out.main)).toEqual(["docs", "src", "README.md", "Cargo.lock"]);
    expect(names(out.worktrees)).toEqual(["@wt/feature-ui", "@wt/code-only"]);
  });

  it("handles a project that is not loaded yet and an empty one", () => {
    expect(filterTree(null, docsOnly, { hideEmptyFolders: true, keep: null }).main).toEqual([]);
    const out = filterTree(dir("", []), docsOnly, { hideEmptyFolders: true, keep: null });
    expect(out.shown).toEqual([]);
    expect(out.hidden).toEqual([]);
  });

  it("does not change the tree it was given", () => {
    const before = JSON.stringify(root);
    filterTree(root, docsOnly, { hideEmptyFolders: true, keep: "src/main.rs" });
    expect(JSON.stringify(root)).toBe(before);
  });
});

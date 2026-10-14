import { describe, expect, it } from "vitest";
import {
  ancestors,
  basename,
  breadcrumbs,
  dirname,
  extname,
  decodeFragment,
  folderLabel,
  isMarkdown,
  resolveRelative,
  splitWorktree,
} from "./paths";

describe("basic helpers", () => {
  it("splits names and folders", () => {
    expect(basename("docs/specs/a.md")).toBe("a.md");
    expect(dirname("docs/specs/a.md")).toBe("docs/specs");
    expect(dirname("a.md")).toBe("");
    expect(extname("Docs/A.MD")).toBe("md");
    expect(extname(".gitignore")).toBe("");
  });

  it("recognizes Markdown documents", () => {
    expect(isMarkdown("a.md")).toBe(true);
    expect(isMarkdown("a.MARKDOWN")).toBe(true);
    expect(isMarkdown("a.json")).toBe(false);
    expect(isMarkdown("README")).toBe(false);
  });
});

describe("worktree paths", () => {
  it("separates the worktree name", () => {
    expect(splitWorktree("@wt/feature-ui/docs/a.md")).toEqual({
      worktree: "feature-ui",
      rel: "docs/a.md",
    });
    expect(splitWorktree("docs/a.md")).toEqual({ worktree: null, rel: "docs/a.md" });
    expect(splitWorktree("@wt/x")).toEqual({ worktree: "x", rel: "" });
  });

  it("labels folders without the worktree prefix", () => {
    expect(folderLabel("@wt/feature-ui/docs/a.md")).toBe("docs");
    expect(folderLabel("@wt/feature-ui/a.md")).toBe("");
    expect(folderLabel("a.md")).toBe("");
  });
});

describe("ancestors and breadcrumbs", () => {
  it("lists the folders to expand, outermost first", () => {
    expect(ancestors("docs/specs/a.md")).toEqual(["docs", "docs/specs"]);
    expect(ancestors("a.md")).toEqual([]);
    expect(ancestors("@wt/x/docs/a.md")).toEqual(["@wt/x", "@wt/x/docs"]);
  });

  it("builds clickable breadcrumb segments", () => {
    expect(breadcrumbs("docs/specs/a.md")).toEqual([
      { name: "docs", path: "docs" },
      { name: "specs", path: "docs/specs" },
      { name: "a.md", path: "docs/specs/a.md" },
    ]);
    expect(breadcrumbs("@wt/x/a.md")[0]).toEqual({ name: "@wt x", path: "@wt/x" });
  });
});

describe("resolveRelative", () => {
  it("resolves against the document's folder", () => {
    expect(resolveRelative("docs/specs/a.md", "img/p.png")).toBe("docs/specs/img/p.png");
    expect(resolveRelative("docs/specs/a.md", "../images/p.svg")).toBe("docs/images/p.svg");
    expect(resolveRelative("a.md", "./p.png")).toBe("p.png");
  });

  it("decodes percent-encoding and backslashes, ignores query and hash", () => {
    expect(resolveRelative("docs/a.md", "my%20pic.png?v=2#x")).toBe("docs/my pic.png");
    expect(resolveRelative("docs/a.md", "img\\p.png")).toBe("docs/img/p.png");
  });

  it("treats a leading slash as relative to the project root", () => {
    expect(resolveRelative("docs/specs/a.md", "/assets/p.png")).toBe("assets/p.png");
  });

  it("returns null for URLs, drive paths and references that escape the root", () => {
    expect(resolveRelative("a.md", "https://example.com/p.png")).toBeNull();
    expect(resolveRelative("a.md", "data:image/png;base64,AAAA")).toBeNull();
    expect(resolveRelative("a.md", "//cdn.example.com/p.png")).toBeNull();
    expect(resolveRelative("a.md", "C:\\pics\\p.png")).toBeNull();
    expect(resolveRelative("a.md", "../outside.png")).toBeNull();
    expect(resolveRelative("a.md", "")).toBeNull();
  });

  it("keeps worktree paths inside their worktree", () => {
    expect(resolveRelative("@wt/x/docs/a.md", "../img/p.png")).toBe("@wt/x/img/p.png");
    expect(resolveRelative("@wt/x/a.md", "../p.png")).toBeNull();
  });
});

describe("decodeFragment", () => {
  it("decodes percent-encoded accents and keeps broken escapes", () => {
    expect(decodeFragment("a%C3%A7%C3%B5es")).toBe("ações");
    expect(decodeFragment("plain-id")).toBe("plain-id");
    expect(decodeFragment("100%")).toBe("100%");
  });
});

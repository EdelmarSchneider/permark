import { describe, expect, it } from "vitest";
import { FILE_CATEGORIES, categoryOf, isFileCategory } from "./fileKinds";

describe("categoryOf", () => {
  it.each([
    ["README.md", "documents"],
    ["docs/guide.markdown", "documents"],
    ["docs/page.mdx", "documents"],
    ["notes.txt", "text"],
    ["docs/spec.rst", "text"],
    ["logo.png", "images"],
    ["art/photo.JPG", "images"],
    ["icon.svg", "images"],
    ["package.json", "data"],
    ["ci/pipeline.yml", "data"],
    ["Cargo.toml", "data"],
    ["data/rows.csv", "data"],
    ["feed.xml", "xml"],
    ["schema.xsd", "xml"],
    ["src/main.rs", "other"],
    ["Dockerfile", "other"],
    [".gitignore", "other"],
    ["archive.tar.gz", "other"],
    ["@wt/feature/docs/a.md", "documents"],
  ])("%s is %s", (path, category) => {
    expect(categoryOf(path)).toBe(category);
  });

  it("ignores dots in folder names and is case-insensitive", () => {
    expect(categoryOf("v1.2/readme")).toBe("other");
    expect(categoryOf("docs/NOTES.MD")).toBe("documents");
  });

  it("covers every category in the list", () => {
    expect(new Set(FILE_CATEGORIES).size).toBe(6);
    expect(isFileCategory("images")).toBe(true);
    expect(isFileCategory("pictures")).toBe(false);
    expect(isFileCategory(3)).toBe(false);
  });
});

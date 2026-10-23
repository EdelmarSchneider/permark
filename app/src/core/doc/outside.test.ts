import { describe, expect, it } from "vitest";
import { outsideTarget } from "./outside";

describe("a link above a single file's folder", () => {
  it("opens the nearest folder holding both the page and the target", () => {
    expect(
      outsideTarget("C:\\Users\\me\\Docs\\notes", "README.md", "../plans/q4.md#goals"),
    ).toEqual({
      folder: "C:/Users/me/Docs",
      name: "Docs",
      document: "plans/q4.md",
      anchor: "goals",
    });
    // from a document further down the folder
    expect(outsideTarget("/home/me/work/app", "docs/guide.md", "../../shared/a%20b.md")).toEqual({
      folder: "/home/me/work",
      name: "work",
      document: "shared/a b.md",
      anchor: null,
    });
  });

  it("a target straight in a folder above: that folder", () => {
    expect(outsideTarget("C:/a/b/c", "x.md", "../y.md")).toMatchObject({
      folder: "C:/a/b",
      document: "y.md",
    });
  });

  it("never offers a whole drive or the file system root", () => {
    expect(outsideTarget("C:/notes", "x.md", "../other/y.md")).toBeNull();
    expect(outsideTarget("/notes", "x.md", "../y.md")).toBeNull();
    expect(outsideTarget("C:/a", "x.md", "../../../../y.md")).toBeNull();
  });

  it("reads Windows drive letters without regard to case", () => {
    expect(outsideTarget("c:/Work/App", "x.md", "../Lib/y.md")?.folder).toBe("c:/Work");
  });

  it("leaves absolute, root-relative and web links alone", () => {
    for (const href of ["/docs/x.md", "D:/x.md", "file:///C:/x.md", "https://x.org", "#a", ""])
      expect(outsideTarget("C:/a/b", "x.md", href), href).toBeNull();
  });
});

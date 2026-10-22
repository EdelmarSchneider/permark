import { describe, expect, it } from "vitest";
import { pictureSource } from "./pictures";

const DOC = "docs/guide/a.md";
const file = (path: string) => ({ kind: "file", path });

describe("pictureSource (the reading view's rule, sanitize.rs)", () => {
  it("joins a relative picture to the document's folder, decoded", () => {
    expect(pictureSource(DOC, "img/a%20b.png")).toEqual(file("docs/guide/img/a b.png"));
    expect(pictureSource(DOC, "./img/a.png")).toEqual(file("docs/guide/img/a.png"));
    expect(pictureSource(DOC, "../x.png")).toEqual(file("docs/x.png"));
    expect(pictureSource(DOC, "/top.png")).toEqual(file("top.png"));
    expect(pictureSource(DOC, "img%5Ca.png")).toEqual(file("docs/guide/img/a.png"));
    expect(pictureSource(DOC, "img\\a.png")).toEqual(file("docs/guide/img/a.png"));
    expect(pictureSource(DOC, "  img/a.png?v=2#top ")).toEqual(file("docs/guide/img/a.png"));
  });

  it("keeps Unicode names and leaves a broken escape as written", () => {
    expect(pictureSource(DOC, "fotos/ação.png")).toEqual(file("docs/guide/fotos/ação.png"));
    expect(pictureSource(DOC, "fotos/a%C3%A7%C3%A3o.png")).toEqual(
      file("docs/guide/fotos/ação.png"),
    );
    expect(pictureSource(DOC, "100%zz.png")).toEqual(file("docs/guide/100%zz.png"));
    expect(pictureSource(DOC, "a%E0%A4%A.png")).toEqual(file("docs/guide/a\uFFFD%A.png"));
  });

  it("gives nothing above the root, for empty values, anchors and other schemes", () => {
    expect(pictureSource(DOC, "../../../x.png")).toEqual({ kind: "none" });
    expect(pictureSource("README.md", "../x.png")).toEqual({ kind: "none" });
    expect(pictureSource(DOC, "")).toEqual({ kind: "none" });
    expect(pictureSource(DOC, "   ")).toEqual({ kind: "none" });
    expect(pictureSource(DOC, "#x")).toEqual({ kind: "none" });
    expect(pictureSource(DOC, "?v=1")).toEqual({ kind: "none" });
    expect(pictureSource(DOC, "javascript:alert(1)")).toEqual({ kind: "none" });
    expect(pictureSource(DOC, "JaVa\tScript:alert(1)")).toEqual({ kind: "none" });
    expect(pictureSource(DOC, "file:///C:/x.png")).toEqual({ kind: "none" });
    expect(pictureSource(DOC, "pm-asset:secret")).toEqual({ kind: "none" });
  });

  it("keeps raster data pictures only", () => {
    const png = "data:image/png;base64,iVBORw0KGgo=";
    expect(pictureSource(DOC, png)).toEqual({ kind: "inline", src: png });
    expect(pictureSource(DOC, "DATA:image/jpeg,xx")).toEqual({
      kind: "inline",
      src: "DATA:image/jpeg,xx",
    });
    expect(pictureSource(DOC, "data:image/svg+xml,<svg/>")).toEqual({ kind: "none" });
    expect(pictureSource(DOC, "da\nta:image/svg+xml,<svg/>")).toEqual({ kind: "none" });
    expect(pictureSource(DOC, "data:text/html,x")).toEqual({ kind: "none" });
  });

  it("names web pictures (never fetched)", () => {
    expect(pictureSource(DOC, "https://example.com/a.png")).toEqual({
      kind: "web",
      url: "https://example.com/a.png",
    });
    expect(pictureSource(DOC, "//cdn.example.com/a.png").kind).toBe("web");
    expect(pictureSource(DOC, "HTTP://x/a.png").kind).toBe("web");
  });

  it("stays inside the worktree a document belongs to", () => {
    const doc = "@wt/feat/docs/a.md";
    expect(pictureSource(doc, "img/x.png")).toEqual(file("@wt/feat/docs/img/x.png"));
    expect(pictureSource(doc, "../x.png")).toEqual(file("@wt/feat/x.png"));
    expect(pictureSource(doc, "/x.png")).toEqual(file("@wt/feat/x.png"));
    expect(pictureSource(doc, "../../x.png")).toEqual({ kind: "none" });
  });

  it("a single file: pictures next to it and below", () => {
    expect(pictureSource("README.md", "img/logo.png")).toEqual(file("img/logo.png"));
  });
});

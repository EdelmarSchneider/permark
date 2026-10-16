import { describe, expect, it } from "vitest";
import {
  FILE_CATEGORIES,
  assetKindOf,
  canOpenInPane,
  categoryOf,
  extensionOf,
  isFileCategory,
  isMediaViewKind,
} from "./fileKinds";

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
    ["talk.mp4", "media"],
    ["sounds/Ping.WAV", "media"],
    ["clip.mkv", "media"],
    ["voice.opus", "media"],
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
    expect(new Set(FILE_CATEGORIES).size).toBe(7);
    expect(isFileCategory("media")).toBe(true);
    expect(isFileCategory("images")).toBe(true);
    expect(isFileCategory("pictures")).toBe(false);
    expect(isFileCategory(3)).toBe(false);
  });
});

describe("what a picture, sound or video is", () => {
  it.each([
    ["logo.png", "image"],
    ["a/b/photo.JPEG", "image"],
    ["icon.ico", "image"],
    ["song.mp3", "audio"],
    ["x.flac", "audio"],
    ["x.weba", "audio"],
    ["movie.mp4", "video"],
    ["movie.MOV", "video"],
    ["movie.mkv", "video"],
    ["old.avi", "video"],
    ["notes.md", null],
    ["data.json", null],
    ["archive.zip", null],
    ["mp4", null],
    [".mp4", null],
    ["name.", null],
  ])("%s is %s", (path, kind) => {
    expect(assetKindOf(path)).toBe(kind);
  });

  it("puts every sound and video in the media category and every picture in images", () => {
    for (const path of [
      "a.mp3",
      "a.wav",
      "a.ogg",
      "a.oga",
      "a.opus",
      "a.flac",
      "a.m4a",
      "a.aac",
      "a.weba",
    ]) {
      expect([assetKindOf(path), categoryOf(path)]).toEqual(["audio", "media"]);
    }
    for (const path of ["a.mp4", "a.m4v", "a.webm", "a.ogv", "a.mov"]) {
      expect([assetKindOf(path), categoryOf(path)]).toEqual(["video", "media"]);
    }
    for (const path of [
      "a.png",
      "a.jpg",
      "a.jpeg",
      "a.gif",
      "a.webp",
      "a.avif",
      "a.bmp",
      "a.ico",
      "a.svg",
    ]) {
      expect([assetKindOf(path), categoryOf(path)]).toEqual(["image", "images"]);
    }
  });

  it("reads the extension of a name, and none from a dotfile or a trailing dot", () => {
    expect(extensionOf("a/b.c/d.TXT")).toBe("txt");
    expect(extensionOf(".gitignore")).toBeNull();
    expect(extensionOf("v1.2/readme")).toBeNull();
    expect(extensionOf("end.")).toBeNull();
  });

  it("opens in the pane what the pane can show: documents, pictures, sound and video", () => {
    for (const path of ["a.md", "docs/b.markdown", "x.png", "y.mp4", "z.flac", "old.avi"]) {
      expect(canOpenInPane(path), path).toBe(true);
    }
    for (const path of ["a.rs", "b.json", "c.pdf", "d.exe", "Makefile"]) {
      expect(canOpenInPane(path), path).toBe(false);
    }
  });

  it("tells the kinds of document view that are drawn from the file", () => {
    for (const kind of ["image", "audio", "video"]) expect(isMediaViewKind(kind)).toBe(true);
    for (const kind of ["markdown", "binary", "tooLarge", "text"])
      expect(isMediaViewKind(kind)).toBe(false);
  });
});

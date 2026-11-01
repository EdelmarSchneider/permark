import { describe, expect, it } from "vitest";
import { clipboardPictures, pictureDisplayUrl } from "./pictures";

function picture(name: string, type: string, bytes: number[]): File {
  return new File([new Uint8Array(bytes)], name, { type });
}

function fileItem(file: File | null): { kind: string; getAsFile: () => File | null } {
  return { kind: "file", getAsFile: () => file };
}

describe("clipboardPictures", () => {
  it("reads a screenshot item when files is empty", () => {
    const shot = picture("image.png", "image/png", [1, 2, 3, 4]);
    const found = clipboardPictures({ files: [], items: [fileItem(shot)] });
    expect(found).toEqual([shot]);
  });

  it("keeps a picture once when the same type and size is a file and an item", () => {
    const onFiles = picture("shot.png", "image/png", [1, 2, 3]);
    const onItems = picture("image.png", "image/png", [9, 9, 9]);
    const found = clipboardPictures({ files: [onFiles], items: [fileItem(onItems)] });
    expect(found).toEqual([onFiles]);
  });

  it("keeps a file and an item when the type or the size differs", () => {
    const png = picture("a.png", "image/png", [1, 2, 3]);
    const gif = picture("b.gif", "image/gif", [1, 2, 3]);
    const bigger = picture("c.png", "image/png", [1, 2, 3, 4]);
    const found = clipboardPictures({
      files: [png],
      items: [fileItem(gif), fileItem(bigger)],
    });
    expect(found).toEqual([png, gif, bigger]);
  });

  it("does not collapse two files of the same type and size", () => {
    const first = picture("a.png", "image/png", [1, 2]);
    const second = picture("b.png", "image/png", [3, 4]);
    const again = picture("c.png", "image/png", [5, 6]);
    const found = clipboardPictures({ files: [first, second], items: [fileItem(again)] });
    expect(found).toEqual([first, second]);
  });

  it("returns nothing for text only", () => {
    const text = {
      kind: "string",
      getAsFile: () => picture("hidden.png", "image/png", [1]),
    };
    expect(clipboardPictures({ files: [], items: [text, fileItem(null)] })).toEqual([]);
    expect(clipboardPictures(null)).toEqual([]);
  });

  it("returns nothing for a type the app does not save", () => {
    const bmp = picture("a.bmp", "image/bmp", [1, 2, 3]);
    const audio = picture("a.mp3", "audio/mpeg", [1]);
    const found = clipboardPictures({
      files: [bmp],
      items: [fileItem(audio), fileItem(bmp)],
    });
    expect(found).toEqual([]);
  });

  it("accepts png, jpg, gif, webp, and svg", () => {
    const files = [
      picture("a.png", "image/png", [1]),
      picture("a.jpg", "image/jpeg", [1]),
      picture("a.gif", "image/gif", [1]),
      picture("a.webp", "image/webp", [1]),
      picture("a.svg", "image/svg+xml", [1]),
    ];
    expect(clipboardPictures({ files, items: [] })).toEqual(files);
  });
});

describe("pictureDisplayUrl", () => {
  const asset = (path: string): string => `asset:${path}`;

  it("resolves a picture next to the document and one in a subfolder", () => {
    expect(pictureDisplayUrl("docs/note.md", "a.png", asset)).toBe("asset:docs/a.png");
    expect(pictureDisplayUrl("docs/note.md", "img/a%20b.png", asset)).toBe(
      "asset:docs/img/a b.png",
    );
  });

  it("keeps a remote address and a data address", () => {
    expect(pictureDisplayUrl("docs/note.md", "https://example.com/a.png", asset)).toBe(
      "https://example.com/a.png",
    );
    expect(pictureDisplayUrl("docs/note.md", "data:image/png,xx", asset)).toBe("data:image/png,xx");
    expect(pictureDisplayUrl("docs/note.md", "blob:https://x/1", asset)).toBe("blob:https://x/1");
  });

  it("reads a pm-asset path and a project-root path", () => {
    expect(pictureDisplayUrl("docs/note.md", "pm-asset:docs/img/a%20b.png", asset)).toBe(
      "asset:docs/img/a b.png",
    );
    expect(pictureDisplayUrl("docs/note.md", "/img/a.png", asset)).toBe("asset:img/a.png");
  });

  it("leaves a path that climbs out of the project", () => {
    expect(pictureDisplayUrl("docs/note.md", "../../x.png", asset)).toBe("../../x.png");
    expect(pictureDisplayUrl("docs/note.md", "", asset)).toBe("");
  });
});

import { describe, expect, it } from "vitest";
import { BOM } from "./blockModel";
import { fromBuffer, toBuffer } from "./bufferText";
import {
  CONFIRM_ABOVE_BYTES,
  imageExtension,
  imageMarkdown,
  isImageName,
  pastedImageName,
} from "./imageName";
import { toggleMarker, wrapLink, type WrapEdit } from "./wrapSelection";

describe("bufferText", () => {
  it("takes the BOM and the line endings out of the buffer and puts them back", () => {
    const text = `${BOM}# Title\r\n\r\nBody\r\n`;
    const { buffer, format } = toBuffer(text);
    expect(buffer).toBe("# Title\n\nBody\n");
    expect(format).toEqual({ bom: true, eol: "\r\n" });
    expect(fromBuffer(buffer, format)).toBe(text);
    expect(fromBuffer(`${buffer}more\n`, format)).toBe(`${text}more\r\n`);
  });

  it("leaves a plain LF file alone", () => {
    const { buffer, format } = toBuffer("a\nb\n");
    expect(buffer).toBe("a\nb\n");
    expect(format).toEqual({ bom: false, eol: "\n" });
    expect(fromBuffer(buffer, format)).toBe("a\nb\n");
  });

  it("gives a file that mixes line endings its most common one", () => {
    const { buffer, format } = toBuffer("a\r\nb\r\nc\nd\r\n");
    expect(format.eol).toBe("\r\n");
    expect(fromBuffer(buffer, format)).toBe("a\r\nb\r\nc\r\nd\r\n");
  });
});

/** Applies an edit to a text, as the editor does. */
const apply = (doc: string, edit: WrapEdit): string =>
  doc.slice(0, edit.from) + edit.insert + doc.slice(edit.to);

describe("toggleMarker", () => {
  it("wraps a selection and selects the same words", () => {
    const edit = toggleMarker("make this bold now", 5, 9, "**");
    expect(apply("make this bold now", edit)).toBe("make **this** bold now");
    expect(edit.selection).toEqual({ anchor: 7, head: 11 });
  });

  it("puts the caret between the markers when nothing is selected", () => {
    const edit = toggleMarker("ab", 1, 1, "**");
    expect(apply("ab", edit)).toBe("a****b");
    expect(edit.selection).toEqual({ anchor: 3, head: 3 });
  });

  it("takes the markers off when they surround the selection", () => {
    const doc = "make **this** bold";
    const edit = toggleMarker(doc, 7, 11, "**");
    expect(apply(doc, edit)).toBe("make this bold");
    expect(edit.selection).toEqual({ anchor: 5, head: 9 });
  });

  it("takes the markers off when the selection includes them", () => {
    const doc = "make **this** bold";
    const edit = toggleMarker(doc, 5, 13, "**");
    expect(apply(doc, edit)).toBe("make this bold");
  });

  it("does not take italic off bold, and wraps bold text in italics", () => {
    const doc = "make **this** bold";
    const edit = toggleMarker(doc, 7, 11, "*");
    expect(apply(doc, edit)).toBe("make ***this*** bold");
    const italic = toggleMarker("a *b* c", 3, 4, "*");
    expect(apply("a *b* c", italic)).toBe("a b c");
  });

  it("wraps with other markers, such as strike", () => {
    expect(apply("x y z", toggleMarker("x y z", 2, 3, "~~"))).toBe("x ~~y~~ z");
  });
});

describe("wrapLink", () => {
  it("makes a link of the selection and selects the placeholder target", () => {
    const edit = wrapLink("see the docs here", 8, 12);
    expect(apply("see the docs here", edit)).toBe("see the [docs](url) here");
    expect("see the [docs](url) here".slice(edit.selection.anchor, edit.selection.head)).toBe(
      "url",
    );
  });

  it("opens an empty link with the caret in the text", () => {
    const edit = wrapLink("ab", 1, 1);
    expect(apply("ab", edit)).toBe("a[]()b");
    expect(edit.selection).toEqual({ anchor: 2, head: 2 });
  });

  it("uses a selected web address as the target", () => {
    const doc = "go to https://example.com now";
    const edit = wrapLink(doc, 6, 25);
    expect(apply(doc, edit)).toBe("go to [](https://example.com) now");
  });
});

describe("imageName", () => {
  it("names a pasted picture after the document and the local time", () => {
    const at = new Date(2026, 9, 14, 10, 42, 1);
    expect(pastedImageName("docs/specs/live-feed.md", "png", at)).toBe(
      "live-feed-20261014-104201.png",
    );
    expect(pastedImageName("README.md", "jpg", new Date(2026, 0, 2, 3, 4, 5))).toBe(
      "README-20260102-030405.jpg",
    );
  });

  it("knows the image types it adds", () => {
    expect(imageExtension("image/png")).toBe("png");
    expect(imageExtension("IMAGE/JPEG")).toBe("jpg");
    expect(imageExtension("image/svg+xml")).toBe("svg");
    expect(imageExtension("application/pdf")).toBeNull();
    expect(isImageName("Shot.PNG")).toBe(true);
    expect(isImageName("photo.jpeg")).toBe(true);
    expect(isImageName("notes.md")).toBe(false);
    expect(isImageName("png")).toBe(false);
  });

  it("writes the Markdown for an image, escaping what would end the link", () => {
    expect(imageMarkdown("assets/shot.png")).toBe("![](assets/shot.png)");
    expect(imageMarkdown("assets/a b (1).png", "alt")).toBe("![alt](assets/a%20b%20%281%29.png)");
  });

  it("asks before adding anything over 10 MB", () => {
    expect(CONFIRM_ABOVE_BYTES).toBe(10 * 1024 * 1024);
  });
});

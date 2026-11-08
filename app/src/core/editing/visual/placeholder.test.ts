// @vitest-environment jsdom
import "./jsdomShims";
import { afterAll, beforeAll, describe, expect, it } from "vitest";
import { createCore, type Core } from "./core";
import { emptyHint } from "./placeholder";

const HINT = "Start writing here. Type / for headings, lists and more.";
let core: Core;
let host: HTMLElement;

beforeAll(async () => {
  host = document.createElement("div");
  document.body.append(host);
  core = await createCore({ host, plugins: [emptyHint(HINT)] });
});

afterAll(async () => {
  await core.destroy();
});

const hinted = (): Element | null => host.querySelector("[data-placeholder]");

describe("Edit on an empty document", () => {
  it("shows a hint where the text goes; it goes away with the first letter and never reaches the file", () => {
    core.setDoc(core.parse(""));
    expect(hinted()?.getAttribute("data-placeholder")).toBe(HINT);
    expect(hinted()?.tagName).toBe("P");
    expect(core.serialize(core.view.state.doc).trim()).toBe("");
    core.view.dispatch(core.view.state.tr.insertText("Hello", 1));
    expect(hinted()).toBeNull();
  });

  it("a document with text shows no hint", () => {
    core.setDoc(core.parse("# Title\n\nSome text.\n"));
    expect(hinted()).toBeNull();
  });
});

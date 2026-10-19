// @vitest-environment jsdom
import { EditorState } from "@codemirror/state";
import { EditorView } from "@codemirror/view";
import { afterEach, describe, expect, it, vi } from "vitest";
import { secretMask, type SecretRange } from "./mask";

// jsdom has no layout
Range.prototype.getBoundingClientRect = () => new DOMRect();
Range.prototype.getClientRects = () => [] as unknown as DOMRectList;

let view: EditorView | null = null;

afterEach(() => {
  view?.destroy();
  view = null;
  document.body.replaceChildren();
});

const TEXT = "A=1\nAPI_KEY=abc123\nB=2\nTOKEN=zzz\n";

/** Where `abc123` and `zzz` are in a text (what the backend answers). */
function rangesOf(text: string): SecretRange[] {
  return ["abc123", "zzz"].flatMap((word) => {
    const at = text.indexOf(word);
    return at < 0 ? [] : [{ from: at, to: at + word.length }];
  });
}

function start(load = (text: string) => Promise.resolve(rangesOf(text))): EditorView {
  const parent = document.createElement("div");
  document.body.append(parent);
  view = new EditorView({
    parent,
    state: EditorState.create({ doc: TEXT, extensions: [secretMask(load, "Show value")] }),
  });
  return view;
}

describe("secret masks in Source", () => {
  it("hides each secret value as soon as the ranges arrive, and keeps the document intact", async () => {
    const v = start();
    // nothing shows before the first answer
    expect(v.dom.classList.contains("cm-pending")).toBe(true);
    await vi.waitFor(() => expect(v.dom.querySelectorAll(".cm-secret")).toHaveLength(2));
    expect(v.dom.classList.contains("cm-pending")).toBe(false);
    expect(v.dom.textContent).not.toContain("abc123");
    expect(v.dom.textContent).not.toContain("zzz");
    expect(v.dom.textContent).toContain("API_KEY=");
    // the text itself is untouched
    expect(v.state.doc.toString()).toBe(TEXT);
  });

  it("shows one value on a click and no other", async () => {
    const v = start();
    await vi.waitFor(() => expect(v.dom.querySelectorAll(".cm-secret")).toHaveLength(2));
    v.dom.querySelector<HTMLButtonElement>(".cm-secret")?.click();
    await vi.waitFor(() => expect(v.dom.querySelectorAll(".cm-secret")).toHaveLength(1));
    expect(v.dom.textContent).toContain("abc123");
    expect(v.dom.textContent).not.toContain("zzz");
  });

  it("follows the edits before the next answer and asks again after typing stops", async () => {
    const load = vi.fn((text: string) => Promise.resolve(rangesOf(text)));
    const v = start(load);
    await vi.waitFor(() => expect(v.dom.querySelectorAll(".cm-secret")).toHaveLength(2));
    expect(load).toHaveBeenCalledTimes(1);
    // a line typed above moves both values; the masks move with them
    v.dispatch({ changes: { from: 0, insert: "NEW=line\n" } });
    expect(v.dom.querySelectorAll(".cm-secret")).toHaveLength(2);
    expect(v.dom.textContent).not.toContain("abc123");
    await vi.waitFor(() => expect(load).toHaveBeenCalledTimes(2), { timeout: 2000 });
    expect(load).toHaveBeenLastCalledWith(v.state.doc.toString());
  });

  it("shows the text anyway when the answer never comes", async () => {
    vi.useFakeTimers();
    try {
      const v = start(() => new Promise(() => {}));
      expect(v.dom.classList.contains("cm-pending")).toBe(true);
      vi.advanceTimersByTime(2000);
      expect(v.dom.classList.contains("cm-pending")).toBe(false);
    } finally {
      vi.useRealTimers();
    }
  });
});

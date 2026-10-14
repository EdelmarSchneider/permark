// @vitest-environment jsdom
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { OutlineModel } from "../outline";
import { EditorOutline, REBUILD_DELAY_MS, type OutlineSink } from "./outlineSync";

type Head = { level: number; text: string };

/** An editor that is only a list of headings and a scroll position. */
function fakeEditor(initial: Head[]) {
  let notify: ((byUser: boolean) => void) | null = null;
  const editor = {
    list: initial,
    current: null as number | null,
    headings: () => editor.list,
    scrollToHeading: vi.fn(),
    currentHeading: () => editor.current,
    focus: vi.fn(),
    onViewport: (handler: (byUser: boolean) => void) => {
      notify = handler;
      return () => {
        notify = null;
      };
    },
    /** The view moved (or the person did). */
    emit: (byUser: boolean) => notify?.(byUser),
    listening: () => notify !== null,
  };
  return editor;
}

function fakeSink() {
  const sink = {
    sets: [] as { path: string; model: OutlineModel }[],
    current: undefined as number | null | undefined,
    set(path: string, model: OutlineModel) {
      sink.sets.push({ path, model });
    },
    setCurrent(index: number | null) {
      sink.current = index;
    },
  } satisfies OutlineSink & Record<string, unknown>;
  return sink;
}

const H = (level: number, text: string): Head => ({ level, text });
const texts = (sink: ReturnType<typeof fakeSink>) =>
  sink.sets[sink.sets.length - 1].model.items.map((i) => i.text);

beforeEach(() => {
  vi.useFakeTimers({
    toFake: ["setTimeout", "clearTimeout", "requestAnimationFrame", "cancelAnimationFrame"],
  });
});
afterEach(() => {
  vi.useRealTimers();
});

describe("EditorOutline", () => {
  it("builds the outline from the editor at once and reports the section in view", () => {
    const editor = fakeEditor([H(1, "Title"), H(2, "One"), H(2, "Two")]);
    editor.current = 1;
    const sink = fakeSink();
    new EditorOutline("docs/a.md", editor, sink).start();
    expect(sink.sets).toHaveLength(1);
    expect(sink.sets[0].path).toBe("docs/a.md");
    expect(texts(sink)).toEqual(["Title", "One", "Two"]);
    expect(sink.sets[0].model.items.map((i) => i.depth)).toEqual([0, 1, 1]);
    expect(sink.current).toBe(1);
  });

  it("rebuilds 300 ms after typing stops, not while it goes on", () => {
    const editor = fakeEditor([H(1, "Title")]);
    const sink = fakeSink();
    const outline = new EditorOutline("a.md", editor, sink);
    outline.start();
    expect(REBUILD_DELAY_MS).toBe(300);

    editor.list = [H(1, "Title"), H(2, "New")];
    outline.changed();
    vi.advanceTimersByTime(200);
    outline.changed(); // still typing
    vi.advanceTimersByTime(200);
    expect(sink.sets).toHaveLength(1); // 400 ms since the first key, 200 since the last
    vi.advanceTimersByTime(100);
    expect(sink.sets).toHaveLength(2);
    expect(texts(sink)).toEqual(["Title", "New"]);
  });

  it("does not rebuild when the headings did not change, but still follows the view", () => {
    const editor = fakeEditor([H(1, "Title"), H(2, "One")]);
    const sink = fakeSink();
    const outline = new EditorOutline("a.md", editor, sink);
    outline.start();
    editor.current = 1;
    outline.changed(); // typing in a paragraph
    vi.advanceTimersByTime(300);
    expect(sink.sets).toHaveLength(1);
    expect(sink.current).toBe(1);
  });

  it("notices a heading that changed its level or its text", () => {
    const editor = fakeEditor([H(2, "One")]);
    const sink = fakeSink();
    const outline = new EditorOutline("a.md", editor, sink);
    outline.start();
    editor.list = [H(3, "One")];
    outline.changed();
    vi.advanceTimersByTime(300);
    expect(sink.sets).toHaveLength(2);
    editor.list = [H(3, "One!")];
    outline.changed();
    vi.advanceTimersByTime(300);
    expect(sink.sets).toHaveLength(3);
  });

  it("scrolls the editor to the heading that was chosen and keeps it as the current section", () => {
    const editor = fakeEditor([H(1, "A"), H(2, "B"), H(2, "C")]);
    const sink = fakeSink();
    const outline = new EditorOutline("a.md", editor, sink);
    outline.start();
    outline.goTo(2);
    expect(editor.scrollToHeading).toHaveBeenCalledWith(2);
    expect(sink.current).toBe(2);

    // the scroll the jump itself causes does not move the choice (near the end of a text the heading cannot reach the top)
    editor.current = 1;
    editor.emit(false);
    vi.advanceTimersByTime(20);
    expect(sink.current).toBe(2);

    // the person scrolls: the section in view decides again
    editor.emit(true);
    vi.advanceTimersByTime(20);
    expect(sink.current).toBe(1);
  });

  it("follows the scroll position, one update per frame", () => {
    const editor = fakeEditor([H(1, "A"), H(2, "B")]);
    const sink = fakeSink();
    const read = vi.spyOn(editor, "currentHeading");
    new EditorOutline("a.md", editor, sink).start();
    read.mockClear();
    editor.current = 1;
    editor.emit(false);
    editor.emit(false);
    editor.emit(false);
    expect(read).not.toHaveBeenCalled();
    vi.advanceTimersByTime(20);
    expect(read).toHaveBeenCalledTimes(1);
    expect(sink.current).toBe(1);
  });

  it("drops the choice when the headings change", () => {
    const editor = fakeEditor([H(1, "A"), H(2, "B")]);
    const sink = fakeSink();
    const outline = new EditorOutline("a.md", editor, sink);
    outline.start();
    outline.goTo(1);
    editor.list = [H(1, "A"), H(2, "B"), H(2, "C")];
    editor.current = 0;
    outline.changed();
    vi.advanceTimersByTime(300);
    expect(sink.current).toBe(0);
  });

  it("stays on the last listed heading when the section in view is beyond the cut", () => {
    const many = Array.from({ length: 1200 }, (_, i) => H(2, `S${i}`));
    const editor = fakeEditor(many);
    editor.current = 1100;
    const sink = fakeSink();
    new EditorOutline("a.md", editor, sink).start();
    expect(sink.sets[0].model.items).toHaveLength(1000);
    expect(sink.sets[0].model.total).toBe(1200);
    expect(sink.current).toBe(999);
  });

  it("reports no section above the first heading", () => {
    const editor = fakeEditor([H(1, "A")]);
    editor.current = null;
    const sink = fakeSink();
    new EditorOutline("a.md", editor, sink).start();
    expect(sink.current).toBeNull();
  });

  it("puts the focus back in the editor", () => {
    const editor = fakeEditor([]);
    new EditorOutline("a.md", editor, fakeSink()).focusDoc();
    expect(editor.focus).toHaveBeenCalled();
  });

  it("does nothing after it is disposed", () => {
    const editor = fakeEditor([H(1, "A")]);
    const sink = fakeSink();
    const outline = new EditorOutline("a.md", editor, sink);
    outline.start();
    outline.changed();
    editor.emit(false);
    outline.dispose();
    expect(editor.listening()).toBe(false);
    editor.list = [H(1, "A"), H(2, "B")];
    outline.changed();
    vi.advanceTimersByTime(1000);
    expect(sink.sets).toHaveLength(1);
  });
});

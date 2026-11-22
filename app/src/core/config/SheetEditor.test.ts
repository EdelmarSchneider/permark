// @vitest-environment jsdom
import { flushSync, mount, tick, unmount } from "svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

// the components talk to the mock backend (one per test file)
vi.mock("../../api", async () => {
  const { createMockApi } = await import("../../api/mock");
  return { api: createMockApi() };
});

const { api } = await import("../../api");
const { mockHash } = await import("../../api/mockEdit");
const { default: SheetEditor } = await import("./SheetEditor.svelte");
const { EditingSession } = await import("../editing/session.svelte");
const { scalarRow } = await import("../testing/sheets");
type Body = import("../../api").Body;

// `api` is the mock created by the factory above
const mock = api as unknown as ReturnType<typeof import("../../api/mock").createMockApi>;

let component: Record<string, unknown> | null = null;
let session: InstanceType<typeof EditingSession>;
let target: HTMLElement;

beforeEach(() => {
  session = new EditingSession(mock);
  session.bind({
    note: () => {},
    fail: () => {},
    saved: () => {},
    open: () => {},
    readingLine: () => 1,
    showReadingLine: () => {},
    savedText: (count) => `saved ${count}`,
  });
  target = document.createElement("div");
  document.body.append(target);
});

afterEach(() => {
  if (component) void unmount(component);
  component = null;
  document.body.replaceChildren();
  vi.restoreAllMocks();
});

async function open(path: string, at?: number, located = false): Promise<HTMLElement> {
  await session.enter("edit", path, "sheet", { kind: "explicit" }, at, true, located);
  const content = session.content;
  if (!content) throw new Error("no content");
  component = mount(SheetEditor, { target, props: { session, content } });
  await vi.waitFor(() => expect(target.querySelector(".sheet")).not.toBeNull());
  return target;
}

function rowOf(key: string): HTMLElement {
  const found = Array.from(target.querySelectorAll<HTMLElement>(".row")).find(
    (row) => row.querySelector(".key")?.textContent?.trim() === key,
  );
  if (!found) throw new Error(`no row ${key}`);
  return found;
}

async function type(key: string, text: string): Promise<void> {
  rowOf(key).querySelector<HTMLButtonElement>(".hit")?.click();
  flushSync();
  const input = rowOf(key).querySelector<HTMLInputElement>("input");
  if (!input) throw new Error("no input");
  input.value = text;
  input.dispatchEvent(new Event("input", { bubbles: true }));
  input.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true }));
  await tick();
}

describe("Edit mode of a config file", () => {
  it("changes one value: the text differs on one line, the document is unsaved, a save sends that line", async () => {
    await open(".env.example");
    const base = session.base?.text ?? "";
    expect(base).toContain("PORT=3000\n");
    await type("PORT", "4000");
    await vi.waitFor(() => expect(session.state.s).toBe("dirty"));
    expect(session.text).toBe(base.replace("PORT=3000", "PORT=4000"));
    // the sheet shows the new value
    await vi.waitFor(() => expect(rowOf("PORT").textContent).toContain("4000"));
    session.save();
    await vi.waitFor(() => expect(session.state.s).toBe("clean"));
    expect(mock.probe.edit.saves).toHaveLength(1);
    const saved = mock.probe.edit.saves[0];
    expect(saved?.path).toBe(".env.example");
    expect(saved?.edits).toHaveLength(1);
    expect(saved?.edits[0]?.insert).toEqual(["PORT=4000"]);
    expect(saved?.edits[0]?.remove).toBe(1);
  });

  it("typing a value back to what it was is no change at all", async () => {
    await open("Cargo.toml");
    await type("version", "9.9.9");
    await vi.waitFor(() => expect(session.state.s).toBe("dirty"));
    await type("version", "0.4.2");
    await vi.waitFor(() => expect(session.state.s).toBe("clean"));
  });

  it("keeps quotes, comments and the other lines while a value changes", async () => {
    await open("docker-compose.yml");
    const base = session.base?.text ?? "";
    await type("image", "nginx:1.28");
    await vi.waitFor(() => expect(session.text).not.toBe(base));
    const before = base.split("\n");
    const after = session.text.split("\n");
    const changed = after.flatMap((line, i) => (line === before[i] ? [] : [[before[i], line]]));
    expect(changed).toEqual([
      ["    image: nginx:1.27   # pinned", "    image: nginx:1.28   # pinned"],
    ]);
  });

  it("a value under a key that sounds secret opens for typing as written", async () => {
    await open(".env.example");
    rowOf("DB_PASSWORD").querySelector<HTMLButtonElement>(".hit")?.click();
    flushSync();
    const input = rowOf("DB_PASSWORD").querySelector<HTMLInputElement>("input");
    expect(input?.type).toBe("text");
    expect(input?.value).toContain("s3cr3t pass");
  });

  it("an ignore file takes a new line and a deleted line, and nothing else moves", async () => {
    await open(".gitignore");
    const base = session.base?.text ?? "";
    const remove = target.querySelector<HTMLButtonElement>("button[aria-label='Delete this line']");
    remove?.click();
    await vi.waitFor(() => expect(session.state.s).toBe("dirty"));
    expect(session.text).toBe(base.replace("node_modules/\n", ""));
    await vi.waitFor(() => expect(target.textContent).not.toContain("node_modules/"));
  });

  it("a change made before the answer to the previous one lands on top of it, in order", async () => {
    await open(".env.example");
    const base = session.base?.text ?? "";
    const real = mock.configEdit;
    let release = (): void => {};
    const gate = new Promise<void>((resolve) => (release = resolve));
    const sent: string[] = [];
    vi.spyOn(mock, "configEdit").mockImplementation(async (path, text, edit) => {
      sent.push(text);
      // the first answer comes late: the second change is made meanwhile
      if (sent.length === 1) await gate;
      return real(path, text, edit);
    });
    const port = /PORT=(\d+)/.exec(base)?.[0] ?? "";
    await type("PORT", "5001");
    await type("NODE_ENV", "production");
    release();
    await vi.waitFor(() =>
      expect(session.text).toBe(
        base.replace(port, "PORT=5001").replace("NODE_ENV=development", "NODE_ENV=production"),
      ),
    );
    expect(sent).toHaveLength(2);
    // the second change starts from the text the first one made
    expect(sent[1]).toContain("PORT=5001");
  });

  it("changes waiting when the file reloads finish on the text they were made for, never on the reloaded one", async () => {
    await open(".env.example");
    const base = session.base?.text ?? "";
    const real = mock.configEdit;
    let release = (): void => {};
    const gate = new Promise<void>((resolve) => (release = resolve));
    const sent: string[] = [];
    let answered = 0;
    vi.spyOn(mock, "configEdit").mockImplementation(async (path, text, edit) => {
      sent.push(text);
      if (sent.length === 1) await gate;
      const result = await real(path, text, edit);
      answered += 1;
      return result;
    });
    await type("PORT", "5001");
    await type("NODE_ENV", "production");
    // another program writes the file while the first answer is on its way: the clean editor reloads
    const theirs = base.replace("DEBUG=false", "DEBUG=true");
    expect(theirs).not.toBe(base);
    mock.probe.edit.files.set(".env.example", theirs);
    const before = session.content?.seq;
    session.onDocumentChanged({
      window: "main",
      path: ".env.example",
      change: "changed",
      version: mockHash(theirs),
    });
    await vi.waitFor(() => expect(session.content?.seq).not.toBe(before));
    // the window makes a new editor for the new content (`{#key content.seq}`)
    if (component) void unmount(component);
    target.replaceChildren();
    const reloaded = session.content;
    if (!reloaded) throw new Error("no content");
    expect(reloaded.text).toBe(theirs);
    component = mount(SheetEditor, { target, props: { session, content: reloaded } });
    await vi.waitFor(() => expect(target.querySelector(".sheet")).not.toBeNull());
    release();
    await vi.waitFor(() => expect(answered).toBe(2));
    // the second change still went out, on top of the first, from the text both were made for
    expect(sent).toHaveLength(2);
    expect(sent[1]).toContain("PORT=5001");
    expect(sent[1]).not.toContain("DEBUG=true");
    // neither answer reaches the reloaded editor: it keeps the file as it is now, nothing unsaved
    await new Promise((resolve) => setTimeout(resolve, 200));
    expect(session.text).toBe(theirs);
    expect(session.current()).toBe(theirs);
    expect(session.state.s).toBe("clean");
    // the next change starts from the reloaded text
    await type("PORT", "6001");
    await vi.waitFor(() => expect(session.state.s).toBe("dirty"));
    expect(sent[2]).toBe(theirs);
    expect(session.text).toContain("DEBUG=true");
    expect(session.text).not.toContain("NODE_ENV=production");
  });
});

describe("an edit on its way while the editor is made again", () => {
  /** The next value change waits for `release` before the backend answers it. */
  function holdNextEdit(): () => void {
    const real = mock.configEdit;
    let release = (): void => {};
    const gate = new Promise<void>((resolve) => (release = resolve));
    vi.spyOn(mock, "configEdit").mockImplementation(async (path, text, edit) => {
      await gate;
      return real(path, text, edit);
    });
    return release;
  }

  it("a switch to Source waits for the answer: Source holds the change, unsaved", async () => {
    await open(".env.example");
    const base = session.base?.text ?? "";
    const port = /PORT=(\d+)/.exec(base)?.[0] ?? "";
    const release = holdNextEdit();
    await type("PORT", "5001");
    const switched = session.switchTo("source");
    release();
    await switched;
    expect(session.mode).toBe("source");
    expect(session.content?.text).toBe(base.replace(port, "PORT=5001"));
    expect(session.state.s).toBe("dirty");
  });

  it("a rename waits for the answer: the editor made for the new path holds the change, unsaved", async () => {
    await open(".env.example");
    const base = session.base?.text ?? "";
    const port = /PORT=(\d+)/.exec(base)?.[0] ?? "";
    const release = holdNextEdit();
    await type("PORT", "5001");
    // another program moves the file while the answer is on its way (the mock's files are back as they were after)
    const files = mock.probe.edit.files;
    const raw = files.get(".env.example") ?? "";
    files.delete(".env.example");
    files.set("config/.env.example", raw);
    try {
      session.onDocumentChanged({
        window: "main",
        path: ".env.example",
        change: "deleted",
        movedTo: "config/.env.example",
      });
      release();
      await vi.waitFor(() => expect(session.content?.path).toBe("config/.env.example"));
      expect(session.content?.text).toBe(base.replace(port, "PORT=5001"));
      expect(session.state.s).toBe("dirty");
    } finally {
      files.delete("config/.env.example");
      files.set(".env.example", raw);
    }
  });

  it("an answer that comes after the editor is gone reaches neither its sheet nor the session", async () => {
    await open(".gitignore");
    const base = session.base?.text ?? "";
    const release = holdNextEdit();
    target.querySelector<HTMLButtonElement>("button[aria-label='Delete this line']")?.click();
    await tick();
    // the editor goes (a reload, a switch); the ignore file's counts are no longer wanted
    if (component) void unmount(component);
    component = null;
    const asked = vi.spyOn(mock, "ignoreMatches");
    release();
    await new Promise((resolve) => setTimeout(resolve, 600));
    expect(asked).not.toHaveBeenCalled();
    expect(session.text).toBe(base);
    expect(session.state.s).toBe("clean");
  });
});

describe("the line of a sheet in Edit mode", () => {
  const ROW = 30;

  /** One card of 60 keys, a key per line. */
  function longSheet(): Body {
    const rows = Array.from({ length: 60 }, (_, i) => ({
      ...scalarRow(`K${i + 1}`),
      line: i + 1,
      end: i + 1,
    }));
    const group = { path: "", title: "", line: 1, end: 60, comment: null, rows };
    return { kind: "sheet", sheet: { kind: "config", groups: [group] } };
  }

  /** The editor's pane: 300 px tall, rows 30 px each, laid out from its top. */
  function pane(): void {
    target.setAttribute("data-scroller", "");
    Object.defineProperty(target, "clientHeight", { configurable: true, value: 300 });
    vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockImplementation(function (
      this: HTMLElement,
    ) {
      if (this === target) return new DOMRect(0, 0, 600, 300);
      const line = Number(this.dataset["line"]);
      return Number.isFinite(line) && this.dataset["line"] !== undefined
        ? new DOMRect(0, ROW * (line - 1) - target.scrollTop, 600, ROW)
        : new DOMRect(0, 0, 0, 0);
    });
    vi.spyOn(mock, "configSheet").mockResolvedValue({ ok: true, value: longSheet() });
  }

  const rowAt = (line: number): HTMLElement | null =>
    target.querySelector<HTMLElement>(`[data-line="${line}"]`);

  it("a line asked for (a search hit) brings its row into view, tinted", async () => {
    pane();
    await open(".env.example");
    session.showLine(40);
    await vi.waitFor(() => expect(rowAt(40)?.classList.contains("pm-located")).toBe(true));
    // the top third of the pane, as the reading view's sheet shows it
    expect(target.scrollTop).toBe(ROW * 39 - 100);
  });

  it("a line asked before the sheet is drawn is shown once it is", async () => {
    pane();
    let release = (): void => {};
    const gate = new Promise<void>((resolve) => (release = resolve));
    const sheet = longSheet();
    vi.spyOn(mock, "configSheet").mockImplementation(async () => {
      await gate;
      return { ok: true, value: sheet };
    });
    await session.enter("edit", ".env.example", "sheet", { kind: "explicit" });
    const content = session.content;
    if (!content) throw new Error("no content");
    component = mount(SheetEditor, { target, props: { session, content } });
    // the editor is attached; its sheet has not come yet
    flushSync();
    session.showLine(40);
    release();
    await vi.waitFor(() => expect(rowAt(40)?.classList.contains("pm-located")).toBe(true));
    expect(target.scrollTop).toBe(ROW * 39 - 100);
  });

  it("opens tinted at a line a place asked for (a search hit in another file)", async () => {
    pane();
    await open(".env.example", 7, true);
    await vi.waitFor(() => expect(rowAt(7)?.classList.contains("pm-located")).toBe(true));
    expect(target.scrollTop).toBe(ROW * 6 - 100);
    // a switch to Source keeps the place, not the tint
    await session.switchTo("source");
    expect(session.content?.located).toBe(false);
  });

  it("opens untinted at a line kept, not asked for", async () => {
    pane();
    await open(".env.example", 7);
    await vi.waitFor(() => expect(target.scrollTop).toBe(ROW * 6));
    expect(rowAt(7)?.classList.contains("pm-located")).toBe(false);
  });

  it("opens at the line it starts at, and tells the line at the top of the pane", async () => {
    pane();
    await open(".env.example", 40);
    // a place kept (a mode switch): the row at the top, not tinted
    await vi.waitFor(() => expect(target.scrollTop).toBe(ROW * 39));
    expect(rowAt(40)?.classList.contains("pm-located")).toBe(false);
    // the person scrolls; Source opens where the sheet is
    target.scrollTop = ROW * 20;
    await session.switchTo("source");
    expect(session.content?.line).toBe(21);
  });
});

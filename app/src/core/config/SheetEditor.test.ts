// @vitest-environment jsdom
import { flushSync, mount, tick, unmount } from "svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

// the components talk to the mock backend (one per test file)
vi.mock("../../api", async () => {
  const { createMockApi } = await import("../../api/mock");
  return { api: createMockApi() };
});

const { api } = await import("../../api");
const { default: SheetEditor } = await import("./SheetEditor.svelte");
const { EditingSession } = await import("../editing/session.svelte");

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
});

async function open(path: string): Promise<HTMLElement> {
  await session.enter("edit", path, "sheet", { kind: "explicit" });
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

  it("a secret value stays masked in the editable sheet and is typed over without being shown", async () => {
    await open(".env.example");
    expect(target.textContent).not.toContain("s3cr3t pass");
    rowOf("DB_PASSWORD").querySelector<HTMLButtonElement>(".hit")?.click();
    flushSync();
    const input = rowOf("DB_PASSWORD").querySelector<HTMLInputElement>("input");
    expect(input?.type).toBe("password");
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
});

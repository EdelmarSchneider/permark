// @vitest-environment jsdom
import { flushSync, mount, tick, unmount } from "svelte";
import { afterEach, describe, expect, it, vi } from "vitest";
import { mockView } from "../../api/mockDocs";
import type { NewValue, Sheet } from "../../api";
import ConfigSheet from "./ConfigSheet.svelte";

let target: HTMLElement;
let component: Record<string, unknown> | null = null;

afterEach(() => {
  if (component) void unmount(component);
  component = null;
  document.body.replaceChildren();
});

/** The real sheet the backend made for a fixture file (`cargo test` writes them). */
function sheetOf(path: string): Sheet {
  const body = mockView(path, [], true)?.body;
  if (body?.kind !== "sheet") throw new Error(`no sheet for ${path}`);
  return body.sheet;
}

type Props = {
  editable?: boolean;
  commit?: (path: string, value: NewValue) => Promise<string | null>;
  remove?: (path: string) => Promise<string | null>;
  add?: (after: string | null, text: string) => Promise<string | null>;
  source?: ((line: number) => void) | null;
  matches?: ReadonlyMap<number, number> | null;
};

async function show(sheet: Sheet, props: Props = {}): Promise<HTMLElement> {
  if (component) void unmount(component);
  component = null;
  target?.remove();
  target = document.createElement("div");
  document.body.append(target);
  component = mount(ConfigSheet, {
    target,
    props: { sheet, editable: false, source: null, ...props },
  });
  await tick();
  return target;
}

function rowOf(root: HTMLElement, key: string): HTMLElement {
  const found = Array.from(root.querySelectorAll<HTMLElement>(".row")).find(
    (row) => row.querySelector(".key")?.textContent?.trim() === key,
  );
  if (!found) throw new Error(`no row ${key}`);
  return found;
}

describe("the sheet of every format", () => {
  it("shows a card per table with its keys and values (TOML)", async () => {
    const root = await show(sheetOf("Cargo.toml"));
    const titles = Array.from(root.querySelectorAll("h3")).map((h) => h.textContent?.trim());
    expect(titles).toEqual([
      "package",
      "dependencies",
      "profile.release",
      "package.metadata.deploy",
    ]);
    expect(rowOf(root, "name").textContent).toContain("docs-site");
    expect(rowOf(root, "authors").querySelectorAll(".chip")).toHaveLength(2);
    expect(
      rowOf(root, "publish").querySelector("[role=switch]")?.getAttribute("aria-checked"),
    ).toBe("false");
    // the comment above a card and the one beside a value are captions
    expect(Array.from(root.querySelectorAll(".caption")).map((c) => c.textContent)).toContain(
      "Release build",
    );
    // an inline table is shown as written, read only, with the reason on its lock
    const inline = rowOf(root, "serde");
    expect(inline.querySelector("pre")?.textContent).toContain("features");
    expect(inline.querySelector(".lock")?.getAttribute("title")).toMatch(/inline table/i);
  });

  it("reads YAML as cards and keeps block text and flow lists read only", async () => {
    const root = await show(sheetOf("docker-compose.yml"));
    const titles = Array.from(root.querySelectorAll("h3")).map((h) => h.textContent?.trim());
    expect(titles).toContain("services.web.environment");
    expect(rowOf(root, "image").textContent).toContain("nginx:1.27");
    expect(rowOf(root, "depends_on").querySelector(".lock")).not.toBeNull();
    expect(rowOf(root, "ports").querySelectorAll(".chip")).toHaveLength(2);
  });

  it("reads INI, dotenv and properties", async () => {
    const ini = await show(sheetOf(".editorconfig"));
    expect(rowOf(ini, "indent_size").textContent).toContain("2");
    expect(rowOf(ini, "root").querySelector("[role=switch]")).not.toBeNull();
    const env = await show(sheetOf(".env.example"));
    expect(rowOf(env, "NODE_ENV").textContent).toContain("development");
    expect(rowOf(env, "APP_NAME").querySelector(".note")?.textContent).toBe("shown in the header");
    const properties = await show(sheetOf("config/app.properties"));
    const titles = Array.from(properties.querySelectorAll("h3")).map((h) => h.textContent?.trim());
    expect(titles).toEqual(["server", "spring", "app"]);
    expect(rowOf(properties, "servlet.context-path").textContent).toContain("/api");
  });
});

describe("values", () => {
  it("show as written, key names that sound secret too", async () => {
    const root = await show(sheetOf(".env.example"));
    expect(rowOf(root, "DB_PASSWORD").textContent).toContain("s3cr3t pass");
    expect(rowOf(root, "API_KEY").textContent).toContain("sk-live-abc123");
    expect(rowOf(root, "BASE_URL").textContent).toContain("${HOST}/api");
    expect(root.querySelector(".reveal")).toBeNull();
    expect(root.textContent).not.toContain("••••••");
  });

  it("show raw values and list items as written", async () => {
    const root = await show(sheetOf("docker-compose.yml"));
    expect(root.textContent).toContain("hunter2-prod");
    expect(root.textContent).toContain("sk-live-9d2c41aa");
    const toml = await show(sheetOf("Cargo.toml"));
    expect(rowOf(toml, "api_token").textContent).toContain("dp_live_4f9a1c77b2");
  });
});

describe("editing a value", () => {
  it("opens on a click, commits on Enter with the kind of value, puts it back on Escape", async () => {
    const commit = vi.fn().mockResolvedValue(null);
    const root = await show(sheetOf("Cargo.toml"), { editable: true, commit });
    const open = (key: string): HTMLInputElement => {
      rowOf(root, key).querySelector<HTMLButtonElement>(".hit")?.click();
      flushSync();
      const input = rowOf(root, key).querySelector<HTMLInputElement>("input");
      if (!input) throw new Error("no input");
      return input;
    };
    let input = open("version");
    expect(input.value).toBe("0.4.2");
    input.value = "0.5.0";
    input.dispatchEvent(new Event("input", { bubbles: true }));
    input.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true }));
    await tick();
    expect(commit).toHaveBeenCalledWith("package.version", { type: "text", text: "0.5.0" });
    // a number goes as a literal
    input = open("opt-level");
    input.value = "2";
    input.dispatchEvent(new Event("input", { bubbles: true }));
    input.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true }));
    await tick();
    expect(commit).toHaveBeenLastCalledWith("profile.release.opt-level", {
      type: "literal",
      text: "2",
    });
    // Escape leaves the value as it was and commits nothing
    commit.mockClear();
    input = open("edition");
    input.value = "2024";
    input.dispatchEvent(new Event("input", { bubbles: true }));
    input.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
    await tick();
    expect(commit).not.toHaveBeenCalled();
    expect(rowOf(root, "edition").querySelector("input")).toBeNull();
    expect(rowOf(root, "edition").textContent).toContain("2021");
  });

  it("flips a switch with a literal and says why a refusal happened", async () => {
    const commit = vi.fn().mockResolvedValueOnce(null).mockResolvedValueOnce("newline");
    const root = await show(sheetOf("Cargo.toml"), { editable: true, commit });
    rowOf(root, "lto").querySelector<HTMLButtonElement>("[role=switch]")?.click();
    await tick();
    expect(commit).toHaveBeenCalledWith("profile.release.lto", { type: "literal", text: "false" });
    rowOf(root, "lto").querySelector<HTMLButtonElement>("[role=switch]")?.click();
    await vi.waitFor(() =>
      expect(rowOf(root, "lto").querySelector("[role=alert]")?.textContent).toMatch(/line break/),
    );
  });

  it("does not open a part that cannot be edited exactly", async () => {
    const commit = vi.fn();
    const root = await show(sheetOf("Cargo.toml"), { editable: true, commit });
    // inline tables are raw rows: no edit target
    expect(rowOf(root, "serde").querySelector(".hit")).toBeNull();
    expect(commit).not.toHaveBeenCalled();
  });

  it("is plain reading when the sheet is not editable", async () => {
    const root = await show(sheetOf("Cargo.toml"), { editable: false });
    expect(root.querySelector(".hit")).toBeNull();
    expect(rowOf(root, "lto").querySelector<HTMLButtonElement>("[role=switch]")?.disabled).toBe(
      true,
    );
  });

  it("jumps to the line in Source", async () => {
    const source = vi.fn();
    const root = await show(sheetOf("Cargo.toml"), { source });
    rowOf(root, "version").querySelector<HTMLButtonElement>(".to-source")?.click();
    expect(source).toHaveBeenCalledWith(4);
  });
});

describe("ignore files", () => {
  const counts = new Map([
    [2, 12],
    [3, 0],
  ]);

  it("lists patterns by their commented block with negation marks and match counts", async () => {
    const root = await show(sheetOf(".gitignore"), { matches: counts });
    const titles = Array.from(root.querySelectorAll("h3")).map((h) => h.textContent?.trim());
    expect(titles).toEqual(["Dependencies", "Build output", "Env files"]);
    const patterns = Array.from(root.querySelectorAll(".pattern")).map((p) =>
      p.textContent?.trim(),
    );
    expect(patterns).toContain("node_modules/");
    expect(patterns).toContain("important.log");
    expect(root.querySelectorAll(".mark").length).toBeGreaterThan(3);
    expect(Array.from(root.querySelectorAll(".mark")).map((m) => m.textContent?.trim())).toContain(
      "!",
    );
    const text = root.querySelectorAll(".count");
    expect(text[0]?.textContent).toContain("12");
    expect(text[1]?.textContent?.toLowerCase()).toContain("no files");
    // a pattern with no answer yet is a placeholder
    expect(text[2]?.textContent?.trim()).toBe("…");
  });

  it("adds, edits and deletes whole lines in Edit mode", async () => {
    const add = vi.fn().mockResolvedValue(null);
    const remove = vi.fn().mockResolvedValue(null);
    const commit = vi.fn().mockResolvedValue(null);
    const root = await show(sheetOf(".gitignore"), { editable: true, add, remove, commit });
    root.querySelector<HTMLButtonElement>("button[aria-label='Delete this line']")?.click();
    await tick();
    expect(remove).toHaveBeenCalledWith("L2");
    root.querySelector<HTMLButtonElement>("button[aria-label='Add a pattern below']")?.click();
    flushSync();
    const field = root.querySelector<HTMLInputElement>("input[aria-label='Add a pattern below']");
    expect(field).not.toBeNull();
    if (field) {
      field.value = "*.tmp";
      field.dispatchEvent(new Event("input", { bubbles: true }));
      field.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true }));
    }
    await tick();
    expect(add).toHaveBeenCalledWith("L2", "*.tmp");
    // the end of the list
    const end = Array.from(root.querySelectorAll<HTMLButtonElement>(".end button")).find((b) =>
      b.textContent?.includes("Add a pattern"),
    );
    end?.click();
    flushSync();
    const last = root.querySelector<HTMLInputElement>(".end input");
    if (last) {
      last.value = "*.bak";
      last.dispatchEvent(new Event("input", { bubbles: true }));
      last.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true }));
    }
    await tick();
    expect(add).toHaveBeenLastCalledWith(null, "*.bak");
  });
});

// @vitest-environment jsdom
// The converter against the mock backend: what the plugin adds and removes, the pure rules, the dialog's state (one
// job at a time, cancel, values sent as written), the dialog itself, and the code block button.
import { flushSync, mount, tick, unmount } from "svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("../../api", async () => {
  const { createMockApi } = await import("../../api/mock");
  return { api: createMockApi() };
});
Object.defineProperty(window, "matchMedia", {
  value: () => ({ matches: false, addEventListener: () => {}, removeEventListener: () => {} }),
});

const { api } = await import("../../api");
const { ContributionRegistry, PluginHost, pluginLoaders, CORE_OWNER } =
  await import("../../core/plugins/registry.svelte");
const { contributions } = await import("../../core/plugins/instance.svelte");
const { ConvertStore, converter } = await import("./store.svelte");
const model = await import("./model");
const { default: ConvertDialog } = await import("./ConvertDialog.svelte");
const { default: CodeBlock } = await import("../highlight/CodeBlock.svelte");
const { NO_REQUIREMENT } = await import("../../core/conditions/available");

const mock = api as unknown as ReturnType<typeof import("../../api/mock").createMockApi>;
const probe = mock.probe.convert;

let component: Record<string, unknown> | null = null;

beforeEach(() => {
  probe.calls.length = 0;
  probe.next = null;
  probe.saved.clear();
});

afterEach(() => {
  if (component) void unmount(component);
  component = null;
  converter.close();
  document.body.replaceChildren();
  vi.unstubAllGlobals();
});

async function until(check: () => boolean, what: string): Promise<void> {
  await vi.waitFor(() => expect(check(), what).toBe(true), { timeout: 10_000, interval: 10 });
}

describe("what the plugin adds", () => {
  it("adds its parts when switched on and leaves nothing when switched off", async () => {
    const registry = new ContributionRegistry();
    const host = new PluginHost(registry, pluginLoaders());
    await host.enable("convert");
    expect([...new Set(registry.ownedBy("convert").map((c) => c.kind))].sort()).toEqual([
      "codeAction",
      "command",
      "docDecoration",
      "selectionMenu",
      "statusItem",
      "treeMenu",
    ]);
    const menu = registry.of("treeMenu").find((c) => c.id === "convert.menu");
    expect(menu?.applies("data/users.json", false)).toBe(true);
    expect(menu?.applies("notes/today.md", false)).toBe(false);
    expect(menu?.applies("data", true)).toBe(false);
    expect(host.disable("convert"), "contributions left after deactivate").toBe(0);
    expect(registry.items).toHaveLength(0);
  });
});

describe("the rules", () => {
  it("knows its sources by extension and by fence language", () => {
    expect(model.sourceOf("data/users.json")).toBe("json");
    expect(model.sourceOf("YML")).toBe("yaml");
    expect(model.sourceOf("x.ndjson")).toBe("jsonl");
    expect(model.sourceOf("rust")).toBeNull();
    expect(model.convertible("a/b.toon")).toBe(true);
    expect(model.convertible("README.md")).toBe(false);
    expect(model.convertible("json")).toBe(false);
  });

  it("says the change from the source and marks estimates", () => {
    expect(model.change(40, 100)).toBe("−60 %");
    expect(model.change(112, 100)).toBe("+12 %");
    expect(model.change(100, 100)).toBe("0 %");
    expect(model.count(1240, true, "en")).toBe("~1,240");
    expect(model.count(1240, false, "en")).toBe("1,240");
  });

  it("turns a table of the page into CSV with quoted cells", () => {
    const table = document.createElement("table");
    table.innerHTML =
      '<thead><tr><th>name</th><th>note</th></tr></thead><tbody><tr><td>Ada</td><td>a, b</td></tr><tr><td>Bob "B"</td><td>x</td></tr></tbody>';
    const rows = model.cellsOf(table);
    expect(rows).toEqual([
      ["name", "note"],
      ["Ada", "a, b"],
      ['Bob "B"', "x"],
    ]);
    expect(model.tableAsCsv(rows)).toBe('name,note\nAda,"a, b"\n"Bob ""B""",x\n');
  });
});

describe("the dialog's state", () => {
  it("reads a file, shows the best target first, copies and saves next to the source", async () => {
    mock.probe.edit.files.set("data/users.json", '[{"id":1}]');
    const writeText = vi.fn().mockResolvedValue(undefined);
    const store = new ConvertStore();
    const opened: string[] = [];
    store.bind((path) => opened.push(path), writeText);
    store.open({ kind: "file", path: "data/users.json" });
    expect(store.state.status).toBe("working");
    await until(() => store.state.status === "ready", "the analysis arrives");
    const s = store.state;
    if (s.status !== "ready") throw new Error("not ready");
    expect(s.selected).toBe("toon");
    await store.act("copy");
    expect(writeText).toHaveBeenCalledWith('toon of [{"id":1}]');
    store.select("yaml");
    await store.act("save");
    expect(probe.saved.get("data/users.yaml")).toBe('yaml of [{"id":1}]');
    await store.act("open");
    expect(opened).toEqual(["data/users (2).yaml"]);
    expect(store.state.status).toBe("closed");
    expect(probe.calls).toContain("close 1");
  });

  it("copies at once, with no confirmation, whatever the values are", async () => {
    const store = new ConvertStore();
    const writeText = vi.fn().mockResolvedValue(undefined);
    store.bind(() => {}, writeText);
    probe.next = {
      status: "ready",
      analysis: {
        from: "json",
        bytes: 20,
        sourceTokens: 8,
        tokenizer: null,
        approx: true,
        notes: [],
        targets: [
          {
            id: "toon",
            ext: "toon",
            tokens: 5,
            fit: "good",
            reason: "smaller",
            best: true,
            preview: "password: x\n",
            notes: [],
          },
        ],
      },
    };
    store.open({ kind: "text", text: '{"password":"x"}', language: "json", beside: "a.md" });
    await until(() => store.state.status === "ready", "ready");
    await store.act("copy");
    expect(writeText).toHaveBeenCalledTimes(1);
    expect(probe.calls.some((call) => call.startsWith("result "))).toBe(true);
  });

  it("a cancelled job stops, and its late answer is ignored", async () => {
    const store = new ConvertStore();
    store.open({ kind: "text", text: "a: 1", language: "yaml", beside: null });
    store.cancel();
    expect(store.state.status).toBe("cancelled");
    await tick();
    await new Promise((resolve) => setTimeout(resolve, 20));
    expect(store.state.status).toBe("cancelled");
    expect(probe.calls).toContain("cancel 1");
  });

  it("a refusal says why", async () => {
    probe.next = { status: "refused", reason: "tooLarge", line: 0, detail: "" };
    converter.open({ kind: "file", path: "huge.json" });
    const target = document.createElement("div");
    document.body.append(target);
    component = mount(ConvertDialog, { target });
    await until(() => converter.state.status === "refused", "refused");
    flushSync();
    expect(target.textContent).toContain("over 50 MB");
  });
});

describe("the dialog", () => {
  it("lists the targets with their tokens, the best one, honest words and the preview", async () => {
    converter.open({
      kind: "text",
      text: '[{"id":1,"name":"Ada"}]',
      language: "json",
      beside: "doc.md",
    });
    const target = document.createElement("div");
    document.body.append(target);
    component = mount(ConvertDialog, { target });
    await until(() => converter.state.status === "ready", "ready");
    flushSync();
    const radios = Array.from(target.querySelectorAll<HTMLButtonElement>('[role="radio"]'));
    expect(radios.map((r) => r.querySelector(".tname")?.textContent)).toEqual([
      "TOON",
      "Compact JSON",
      "JSON Lines",
      "CSV",
      "TSV",
      "YAML",
      "Indented JSON",
    ]);
    expect(radios[0]?.getAttribute("aria-checked")).toBe("true");
    expect(radios[0]?.textContent).toContain("best");
    expect(radios[0]?.textContent).toMatch(/~\d/);
    expect(radios[0]?.textContent).toContain("−");
    expect(radios[3]?.disabled).toBe(true);
    expect(radios[3]?.textContent).toContain("Not available: the data is not one flat table");
    expect(radios[6]?.textContent).toContain("Does not help: for reading");
    expect(target.querySelector("pre")?.textContent).toContain("toon of");
    expect(target.textContent).toContain("~ = estimated tokens");
    radios[5]?.click();
    flushSync();
    expect(target.querySelector("pre")?.textContent).toContain("yaml of");
    // Escape closes it
    target
      .querySelector('[role="dialog"]')
      ?.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
    flushSync();
    expect(converter.state.status).toBe("closed");
  });
});

describe("the code block button", () => {
  it("shows the plugin's action next to Copy for the languages it takes", async () => {
    const run = vi.fn();
    const ArrowLeftRight = (await import("@lucide/svelte/icons/arrow-left-right")).default;
    const off = contributions.register({
      owner: CORE_OWNER,
      kind: "codeAction",
      id: "test.convert",
      label: "convert.menu",
      icon: ArrowLeftRight,
      requires: NO_REQUIREMENT,
      applies: (language) => language === "json",
      run,
    });
    try {
      const target = document.createElement("div");
      document.body.append(target);
      component = mount(CodeBlock, {
        target,
        props: { source: '{"a":1}', info: "JSON", display: false },
      });
      flushSync();
      const button = target.querySelector<HTMLButtonElement>(".code-action");
      expect(button?.title).toBe("Convert…");
      button?.click();
      expect(run).toHaveBeenCalledWith('{"a":1}', "json");
      void unmount(component);
      component = mount(CodeBlock, {
        target,
        props: { source: "x", info: "rust", display: false },
      });
      flushSync();
      expect(target.querySelector(".code-action")).toBeNull();
    } finally {
      off();
    }
  });
});

describe("the table button", () => {
  it("shows over a table of the page, goes off it, and sends the table as CSV", async () => {
    const { decorateTables } = await import("./tables");
    const root = document.createElement("div");
    root.innerHTML =
      "<p>text</p><table><tr><th>a</th><th>b</th></tr><tr><td>1</td><td>x, y</td></tr></table>";
    document.body.append(root);
    const opened: string[] = [];
    const stop = decorateTables(root, (csv) => opened.push(csv));
    const button = document.querySelector<HTMLButtonElement>(".pm-convert-table");
    const over = (el: Element | null): void => {
      el?.dispatchEvent(new MouseEvent("pointerover", { bubbles: true }));
    };
    expect(button?.hidden).toBe(true);
    over(root.querySelector("td"));
    expect(button?.hidden).toBe(false);
    over(root.querySelector("p"));
    expect(button?.hidden).toBe(true);
    over(root.querySelector("td"));
    button?.click();
    expect(opened).toEqual(['a,b\n1,"x, y"\n']);
    stop();
    expect(document.querySelector(".pm-convert-table")).toBeNull();
  });
});

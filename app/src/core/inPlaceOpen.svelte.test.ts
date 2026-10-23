// @vitest-environment jsdom
// Another folder opened in this tab (setting "Open folders and files in" = This tab), the whole window on the mock: the
// backend's Opening state comes first (the folder shown before stays on screen, project calls are refused as the
// backend does), then the new folder. Nothing on screen may call for the folder about to go -> no error notice.
import { flushSync, mount, unmount } from "svelte";
import { afterEach, beforeAll, describe, expect, it, vi } from "vitest";
import { MOCK_FOLDER } from "../api/mock";

vi.mock("../api", async () => {
  const { createMockApi } = await import("../api/mock");
  return { api: createMockApi() };
});
Object.defineProperty(window, "matchMedia", {
  value: () => ({ matches: false, addEventListener: () => {}, removeEventListener: () => {} }),
});
Element.prototype.scrollIntoView = () => {};
globalThis.ResizeObserver ??= class {
  observe(): void {}
  unobserve(): void {}
  disconnect(): void {}
};

const { api } = await import("../api");
const { windowStore: store } = await import("./window.svelte");
const { default: App } = await import("../App.svelte");
const { conditions } = await import("./conditions/context.svelte");
const { host } = await import("./plugins/instance.svelte");

const mock = api as unknown as ReturnType<typeof import("../api/mock").createMockApi>;
const OTHER = "C:/Users/demo/plain folder";
const THIRD = "C:/Users/demo/third";

let app: Record<string, unknown> | null = null;

async function until(check: () => boolean, what: string): Promise<void> {
  await vi.waitFor(() => expect(check(), what).toBe(true), { timeout: 10_000, interval: 10 });
}

function rootShown(): string | null {
  return store.view.state === "showing" && store.view.workspace.kind === "project"
    ? store.view.workspace.root
    : null;
}

/** The notices that tell an error (the switch must leave none). */
function errorNotices(): string[] {
  return store.notices.flatMap((n) =>
    n.kind === "openFailed" || n.kind === "apiFailed" || n.kind === "fileOpFailed"
      ? [JSON.stringify(n)]
      : [],
  );
}

beforeAll(async () => {
  await store.start();
  await api.settingsSet({ key: "openIn", value: "this" });
  const target = document.createElement("div");
  document.body.append(target);
  app = mount(App, { target });
});

afterEach(() => {
  mock.probe.opening.ms = 0;
  mock.probe.opening.fails.clear();
  mock.probe.opening.refused.length = 0;
  for (const n of store.notices) store.dismiss(n.id);
});

/** Every plugin that is on loaded, then no plugin call for 300 ms (modules and panels load lazily: in a first run of
 *  the window a plugin may start late, while another folder already opens). */
async function settled(): Promise<void> {
  await until(
    () => conditions.current.plugins.every((id) => host.state(id) !== "loading"),
    "the plugins loaded",
  );
  let seen = -1;
  while (seen !== mock.probe.plugins.calls.length) {
    seen = mock.probe.plugins.calls.length;
    await new Promise((r) => setTimeout(r, 300));
  }
}

/** The demo folder on screen with a document open (the Explorer keeps it listed). */
async function onDemoFolderWithDocument(): Promise<void> {
  await store.open(MOCK_FOLDER, "thisTab");
  await until(() => rootShown() === MOCK_FOLDER, "the demo folder shows");
  await api.showDocument("README.md");
  await until(
    () => store.view.state === "showing" && store.view.document === "README.md",
    "README.md shows",
  );
  await until(() => document.querySelector(".tree, [role='tree']") !== null, "the tree shows");
  // what follows the folder settled (plugins loaded and started, tree open)
  await settled();
  flushSync();
  mock.probe.opening.refused.length = 0;
  for (const n of store.notices) store.dismiss(n.id);
}

describe("another folder in this tab", () => {
  it("switches with no call for the old folder while the new one opens, and no error notice", async () => {
    await onDemoFolderWithDocument();
    mock.probe.opening.ms = 40;
    const switching = store.open(OTHER, "thisTab");
    await until(() => store.view.state === "opening", "the view is opening");
    // the old folder and its document stay on screen while the new one opens
    expect(store.screen?.document).toBe("README.md");
    await switching;
    await until(() => rootShown() === OTHER, "the other folder shows");
    await new Promise((r) => setTimeout(r, 50));
    expect(mock.probe.opening.refused).toEqual([]);
    expect(errorNotices()).toEqual([]);
  });

  it("two fast switches: the newer wins, the older is quiet, nothing refused", async () => {
    await onDemoFolderWithDocument();
    mock.probe.opening.ms = 30;
    const first = store.open(OTHER, "thisTab");
    await until(() => store.view.state === "opening", "the first is opening");
    const second = store.open(THIRD, "thisTab");
    await Promise.all([first, second]);
    await until(() => rootShown() === THIRD, "the third folder shows");
    await new Promise((r) => setTimeout(r, 50));
    expect(mock.probe.opening.refused).toEqual([]);
    expect(errorNotices()).toEqual([]);
  });

  it("a folder that fails to open: back on the old one, and the failure is told", async () => {
    await onDemoFolderWithDocument();
    mock.probe.opening.fails.add(OTHER);
    await store.open(OTHER, "thisTab");
    await until(() => rootShown() === MOCK_FOLDER, "back on the demo folder");
    await new Promise((r) => setTimeout(r, 50));
    expect(mock.probe.opening.refused).toEqual([]);
    const told = errorNotices();
    expect(told.length).toBeGreaterThan(0);
    expect(told.every((n) => n.includes(OTHER) && n.includes('"io"'))).toBe(true);
  });
});

describe("after", () => {
  it("unmounts", () => {
    if (app) unmount(app);
    app = null;
  });
});

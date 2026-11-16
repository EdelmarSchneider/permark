// @vitest-environment jsdom
// The demo pages offer on the Welcome screen against the mock backend: shown until used or dismissed, "Explore" copies
// and opens the folder, "Open" when the folder is in Documents already, a refused copy says why.
import { flushSync, mount, unmount } from "svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { MOCK_DEMO_FOLDER } from "../../api/mock";

vi.mock("../../api", async () => {
  const { createMockApi } = await import("../../api/mock");
  return { api: createMockApi() };
});
Object.defineProperty(window, "matchMedia", {
  value: () => ({ matches: false, addEventListener: () => {}, removeEventListener: () => {} }),
});

const { api } = await import("../../api");
const { default: Welcome } = await import("./Welcome.svelte");
const { windowStore } = await import("../window.svelte");

const mock = api as unknown as ReturnType<typeof import("../../api/mock").createMockApi>;
const store = windowStore;
let target: HTMLElement;
let component: Record<string, unknown> | null = null;
let started = false;

async function until(check: () => boolean, what: string): Promise<void> {
  await vi.waitFor(() => expect(check(), what).toBe(true), { timeout: 10_000, interval: 10 });
}

async function toWelcome(): Promise<void> {
  const listed = await mock.listWindows();
  for (const tab of listed.ok ? (listed.value[0]?.tabs ?? []) : []) await mock.tabClose(tab.tab);
  await until(() => store.view.state === "welcome", "welcome");
}

function card(): HTMLElement | null {
  return target.querySelector<HTMLElement>(".demo");
}

async function showWelcome(): Promise<void> {
  if (!started) {
    await store.start();
    started = true;
  }
  await toWelcome();
  component = mount(Welcome, { target, props: { store } });
  flushSync();
}

beforeEach(() => {
  document.body.replaceChildren();
  target = document.createElement("div");
  document.body.append(target);
  mock.probe.demo.installs = 0;
  mock.probe.demo.dismissed = 0;
  mock.probe.demo.fail = null;
});

afterEach(async () => {
  if (component) await unmount(component);
  component = null;
});

describe("the demo pages offer", () => {
  it("Explore copies the pages, opens their folder as a project and is not offered again", async () => {
    mock.probe.demo.offer = "install";
    await showWelcome();
    await until(() => card() !== null, "offered");
    const main = card()?.querySelector<HTMLButtonElement>("button.demo-use");
    expect(main?.textContent).toContain("Explore the demo pages");
    expect(main?.textContent).toContain("Documents");
    main?.click();
    await until(() => mock.probe.demo.installs === 1, "copied");
    await until(
      () =>
        store.view.state === "showing" &&
        store.view.workspace.kind === "project" &&
        store.view.workspace.root === MOCK_DEMO_FOLDER,
      "the folder opens as a project",
    );
    expect(card()).toBeNull();
    // a later Welcome: used, so nothing
    if (component) await unmount(component);
    await showWelcome();
    await new Promise((r) => setTimeout(r, 20));
    flushSync();
    expect(card()).toBeNull();
  });

  it("says Open when the folder is in Documents already, and opens it", async () => {
    mock.probe.demo.offer = "open";
    await showWelcome();
    await until(() => card() !== null, "offered");
    const main = card()?.querySelector<HTMLButtonElement>("button.demo-use");
    expect(main?.textContent).toContain("Open the demo pages");
    main?.click();
    await until(
      () => store.view.state === "showing" && store.view.workspace.kind === "project",
      "opened",
    );
    expect(mock.probe.demo.installs).toBe(1);
  });

  it("is dismissed for good with its close button", async () => {
    mock.probe.demo.offer = "install";
    await showWelcome();
    await until(() => card() !== null, "offered");
    const close = card()?.querySelector<HTMLButtonElement>('button[aria-label="Dismiss"]');
    expect(close).toBeTruthy();
    expect(close?.type).toBe("button");
    close?.click();
    flushSync();
    expect(card()).toBeNull();
    await until(() => mock.probe.demo.dismissed === 1, "remembered");
    expect(store.view.state).toBe("welcome");
    if (component) await unmount(component);
    await showWelcome();
    await new Promise((r) => setTimeout(r, 20));
    flushSync();
    expect(card()).toBeNull();
  });

  it("a refused copy says why and stays offered", async () => {
    mock.probe.demo.offer = "install";
    mock.probe.demo.fail = "denied";
    await showWelcome();
    await until(() => card() !== null, "offered");
    card()?.querySelector<HTMLButtonElement>("button.demo-use")?.click();
    await until(() => card()?.querySelector('[role="alert"]') !== null, "told");
    expect(card()?.querySelector('[role="alert"]')?.textContent).toContain(
      "Can't copy the demo pages",
    );
    expect(card()?.querySelector("button.demo-use")?.hasAttribute("disabled")).toBe(false);
    expect(store.view.state).toBe("welcome");
  });

  it("shows nothing when there is nothing to offer", async () => {
    mock.probe.demo.offer = "hidden";
    await showWelcome();
    await new Promise((r) => setTimeout(r, 20));
    flushSync();
    expect(card()).toBeNull();
  });
});

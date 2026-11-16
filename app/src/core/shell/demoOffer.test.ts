// @vitest-environment jsdom
// The demo pages offer on the Welcome screen against the mock backend: shown until used or dismissed, "Explore" copies
// and opens the folder, "Open" when the folder is in Documents already, a refused copy says why. Past the offer, the
// palette command and the quick settings entry open the pages whenever this copy ships them.
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
const { default: SettingsPopover } = await import("./SettingsPopover.svelte");
const { demoOffer } = await import("./demoOffer.svelte");
const { ContributionRegistry } = await import("../plugins/registry.svelte");
const { registerCoreCommands } = await import("../commands/core");

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
  mock.probe.demo.available = true;
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

describe("the lasting way to the demo pages", () => {
  async function started(): Promise<void> {
    if (!started_) {
      await store.start();
      started_ = true;
    }
  }
  let started_ = false;

  function demoCommand() {
    const registry = new ContributionRegistry();
    const off = registerCoreCommands(registry);
    const command = registry.of("command").find((c) => c.id === "app.openDemo");
    return { command, off };
  }

  function popoverEntry(): HTMLButtonElement | null {
    return (
      Array.from(target.querySelectorAll<HTMLButtonElement>("button")).find(
        (b) => b.textContent?.trim() === "Open the demo pages",
      ) ?? null
    );
  }

  it("the palette command is offered while the pages ship, also after the offer was dismissed", async () => {
    const { command, off } = demoCommand();
    expect(command?.label).toBe("welcome.demo.open");
    mock.probe.demo.offer = "install";
    await demoOffer.load();
    expect(command?.when?.()).toBe(true);
    await demoOffer.dismiss();
    await demoOffer.load();
    expect(demoOffer.card.status).toBe("hidden");
    expect(command?.when?.()).toBe(true);
    mock.probe.demo.available = false;
    await demoOffer.load();
    expect(command?.when?.()).toBe(false);
    off();
  });

  it("the command copies the pages first, then opens their folder as a project", async () => {
    await started();
    await toWelcome();
    mock.probe.demo.offer = "hidden";
    await demoOffer.load();
    const order: string[] = [];
    const install = vi.spyOn(mock, "demoInstall");
    install.mockImplementation(async () => {
      order.push("install");
      install.mockRestore();
      return mock.demoInstall();
    });
    const opened = vi.spyOn(store, "open");
    opened.mockImplementation(async (path, placement) => {
      order.push(`open ${path}`);
      opened.mockRestore();
      return store.open(path, placement);
    });
    const { command, off } = demoCommand();
    await command?.run();
    expect(order).toEqual(["install", `open ${MOCK_DEMO_FOLDER}`]);
    await until(
      () =>
        store.view.state === "showing" &&
        store.view.workspace.kind === "project" &&
        store.view.workspace.root === MOCK_DEMO_FOLDER,
      "the folder opens as a project",
    );
    off();
  });

  it("the quick settings entry runs the same action and closes the popover", async () => {
    await started();
    await toWelcome();
    await demoOffer.load();
    const close = vi.fn();
    component = mount(SettingsPopover, { target, props: { store, close } });
    flushSync();
    const entry = popoverEntry();
    expect(entry).not.toBeNull();
    entry?.click();
    expect(close).toHaveBeenCalled();
    await until(() => mock.probe.demo.installs === 1, "copied");
    await until(
      () => store.view.state === "showing" && store.view.workspace.kind === "project",
      "opened",
    );
  });

  it("the quick settings show no entry when this copy has no pages", async () => {
    await started();
    mock.probe.demo.available = false;
    await demoOffer.load();
    component = mount(SettingsPopover, { target, props: { store, close: () => {} } });
    flushSync();
    expect(popoverEntry()).toBeNull();
  });

  it("a refused copy is told by a notice, in the card's words", async () => {
    await started();
    await demoOffer.load();
    mock.probe.demo.fail = "denied";
    const { command, off } = demoCommand();
    await command?.run();
    const said = store.notices.some(
      (n) => n.kind === "action" && n.text.includes("Can't copy the demo pages"),
    );
    expect(said).toBe(true);
    off();
  });
});

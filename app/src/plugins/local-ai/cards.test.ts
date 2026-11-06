// @vitest-environment jsdom
// The This computer and Hugging Face token cards against the mock backend: memory, processor, graphics (own memory,
// shared, unknown; software renderers left out); the token card only while a listed model needs one, Save (field
// emptied, never shown again), Clear, refusals in words; the page's card order.
import { flushSync, mount, unmount } from "svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { MachineProfile } from "../../api/generated/MachineProfile";

vi.mock("../../api", async () => {
  const { createMockApi } = await import("../../api/mock");
  return { api: createMockApi() };
});

const { api } = await import("../../api");
const { MOCK_MACHINE } = await import("../../api/mockLocalAi");
const { localAi } = await import("./store.svelte");
const { default: LocalAiSection } = await import("./LocalAiSection.svelte");

const mock = api as unknown as ReturnType<typeof import("../../api/mock").createMockApi>;
const probe = mock.probe.localAi;
const SECRET = "hf_secretvalue123";

let target: HTMLElement;
let component: Record<string, unknown> | null = null;

async function until(check: () => boolean, what: string): Promise<void> {
  await vi.waitFor(() => expect(check(), what).toBe(true), { timeout: 10_000, interval: 10 });
}

function card(title: string): HTMLElement | null {
  return (
    Array.from(target.querySelectorAll<HTMLElement>("section")).find(
      (s) => s.querySelector("h3")?.textContent === title,
    ) ?? null
  );
}

function textOf(title: string): string {
  return (card(title)?.textContent ?? "").replace(/\s+/g, " ");
}

async function openPage(): Promise<void> {
  component = mount(LocalAiSection, { target });
  flushSync();
  await until(() => target.querySelector("li[data-model]") !== null, "the page shows");
  await until(() => localAi.machine !== null, "this computer is read");
  flushSync();
}

async function reopen(): Promise<void> {
  if (component) await unmount(component);
  component = null;
  localAi.stop();
  target.replaceChildren();
  await openPage();
}

beforeEach(() => {
  document.body.replaceChildren();
  target = document.createElement("div");
  document.body.append(target);
  probe.machine = structuredClone(MOCK_MACHINE);
});

afterEach(async () => {
  if (component) await unmount(component);
  component = null;
  localAi.stop();
  probe.stopAll();
  vi.restoreAllMocks();
});

describe("the page", () => {
  it("shows the cards in order: jobs, models, this computer, storage, token", async () => {
    await openPage();
    const titles = Array.from(target.querySelectorAll("section > h3")).map((h) => h.textContent);
    expect(titles).toEqual([
      "What each job uses",
      "Models",
      "This computer",
      "Storage",
      "Hugging Face token",
    ]);
  });
});

describe("This computer", () => {
  it("shows memory (free too), processor threads and the graphics cards with their memory", async () => {
    await openPage();
    const text = textOf("This computer");
    expect(text).toContain("6 GB (2.5 GB free now)");
    expect(text).toContain("8 threads");
    expect(text).toContain("Intel(R) Iris(R) Xe Graphics");
    expect(text).toContain("128 MB of its own memory");
    expect(text).toContain("NVIDIA GeForce MX450");
    expect(text).toContain("2 GB of its own memory");
    // the software renderer is no graphics card
    expect(text).not.toContain("Microsoft Basic Render Driver");
  });

  it("says when graphics share the computer's memory, and what the system does not tell", async () => {
    const machine: MachineProfile = {
      ramTotal: null,
      ramAvailable: null,
      cpuLogical: null,
      gpus: [
        { name: "Apple M3", dedicatedVram: null, shared: null, unified: true, software: false },
        { name: "Odd adapter", dedicatedVram: null, shared: null, unified: false, software: false },
      ],
      os: "macos",
    };
    probe.machine = machine;
    await openPage();
    const text = textOf("This computer");
    expect(text).toContain("Memory Unknown");
    expect(text).toContain("Processor Unknown");
    expect(text).toContain("Apple M3 Shares the computer's memory");
    expect(text).toContain("Odd adapter Memory unknown");
  });

  it("says so when there is no graphics card", async () => {
    probe.machine = { ...structuredClone(MOCK_MACHINE), ramAvailable: null, gpus: [] };
    await openPage();
    const text = textOf("This computer");
    expect(text).toContain("No graphics card found");
    expect(text).not.toContain("free now");
  });
});

describe("Hugging Face token", () => {
  function input(): HTMLInputElement {
    const found = card("Hugging Face token")?.querySelector<HTMLInputElement>("input");
    if (!found) throw new Error("no token field");
    return found;
  }

  function status(): string {
    return (
      card("Hugging Face token")?.querySelector("[data-token-status]")?.textContent?.trim() ?? ""
    );
  }

  function type(value: string): void {
    input().value = value;
    input().dispatchEvent(new Event("input", { bubbles: true }));
    flushSync();
  }

  function press(action: string): void {
    const found = card("Hugging Face token")?.querySelector<HTMLButtonElement>(
      `[data-action="${action}"]`,
    );
    if (!found) throw new Error(`no ${action}`);
    found.click();
    flushSync();
  }

  const alert = (): string =>
    card("Hugging Face token")?.querySelector('[role="alert"]')?.textContent ?? "";

  it("is left out while no listed model needs a token", async () => {
    await openPage();
    expect(card("Hugging Face token")).not.toBeNull();
    const catalog = localAi.catalog;
    if (catalog === null) throw new Error("no catalog");
    localAi.catalog = { ...catalog, models: catalog.models.filter((m) => !m.gated) };
    flushSync();
    expect(card("Hugging Face token")).toBeNull();
  });

  it("has a masked, labeled field and says no token is saved", async () => {
    await openPage();
    expect(status()).toBe("No token");
    expect(input().type).toBe("password");
    expect(input().getAttribute("autocomplete")).toBe("off");
    const label = target.querySelector(`label[for="${input().id}"]`);
    expect(label?.textContent).toBe("Token");
    expect(textOf("Hugging Face token")).toContain("password store");
    expect(card("Hugging Face token")?.querySelector('[data-action="clearToken"]')).toBeNull();
  });

  it("says in words why a token is refused", async () => {
    await openPage();
    press("saveToken");
    await until(() => alert() === "Paste a token first.", "empty refused");
    type("x".repeat(2000));
    press("saveToken");
    await until(() => alert().startsWith("This token is too long."), "too long refused");
    expect(localAi.state?.tokenSet).toBe(false);
  });

  it("saves a token, empties the field and never shows it again; Clear removes it", async () => {
    await openPage();
    type(SECRET);
    press("saveToken");
    await until(
      () => localAi.state?.tokenSet === true && input().value === "",
      "saved, field empty",
    );
    flushSync();
    expect(probe.calls).toContain("setToken");
    expect(status()).toBe("Token saved");
    expect(alert()).toBe("");
    expect(target.innerHTML).not.toContain(SECRET);

    press("clearToken");
    await until(() => localAi.state?.tokenSet === false, "cleared");
    flushSync();
    expect(probe.calls).toContain("clearToken");
    expect(status()).toBe("No token");
  });

  it("follows a token saved elsewhere", async () => {
    await openPage();
    await localAi.setToken(SECRET);
    flushSync();
    expect(status()).toBe("Token saved");
    await localAi.clearToken();
    await reopen();
    expect(status()).toBe("No token");
  });
});

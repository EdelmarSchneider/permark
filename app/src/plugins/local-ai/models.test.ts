// @vitest-environment jsdom
// The Models card against the mock backend: order, what a row says, Details, each install state and its actions
// (Get, terms, progress, Cancel | Resume | Delete partial, Try again, Update, Remove), the live region (milestones
// only), the milestone rule itself. One mock per file: the tests share its installs, so each acts on its own model.
import { flushSync, mount, unmount } from "svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { InstallState } from "../../api/generated/InstallState";

vi.mock("../../api", async () => {
  const { createMockApi } = await import("../../api/mock");
  return { api: createMockApi() };
});

const { api } = await import("../../api");
const { MOCK_MACHINE } = await import("../../api/mockLocalAi");
const { localAi, milestoneOf } = await import("./store.svelte");
const { default: LocalAiSection } = await import("./LocalAiSection.svelte");

const mock = api as unknown as ReturnType<typeof import("../../api/mock").createMockApi>;
const probe = mock.probe.localAi;

const EMBED = "embeddinggemma-2-q8_0";
const GEMMA_E2B = "gemma-4-e2b-it-q4_0";
const GEMMA_E4B = "gemma-4-e4b-it-q4_0";
const LFM = "lfm3-1.7b-q4_k_m";
const WHISPER = "whisper-large-v3-turbo-q5_0";
const GATED = "gemma-3-270m-it-q8_0";
const FLAKY = "embeddinggemma-300m-q4_0";
const LONG = "fachtext-uebersetzer-q4_0";
const LONG_NAME = "Übersetzungsmodell für wissenschaftliche Fachtexte";

let target: HTMLElement;
let component: Record<string, unknown> | null = null;

async function until(check: () => boolean, what: string): Promise<void> {
  await vi.waitFor(() => expect(check(), what).toBe(true), { timeout: 10_000, interval: 10 });
}

function row(model: string): HTMLLIElement {
  const found = target.querySelector<HTMLLIElement>(`li[data-model="${model}"]`);
  if (!found) throw new Error(`no row for ${model}`);
  return found;
}

function action(model: string, name: string): HTMLButtonElement | null {
  return row(model).querySelector<HTMLButtonElement>(`[data-action="${name}"]`);
}

function press(model: string, name: string): HTMLButtonElement {
  const button = action(model, name);
  if (!button) throw new Error(`no ${name} on ${model}: ${row(model).textContent ?? ""}`);
  button.click();
  flushSync();
  return button;
}

function escape(from: Element): void {
  from.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
  flushSync();
}

const text = (model: string): string => (row(model).textContent ?? "").replace(/\s+/g, " ");

async function openPage(): Promise<void> {
  component = mount(LocalAiSection, { target });
  flushSync();
  await until(() => target.querySelector("li[data-model]") !== null, "the models card shows");
  await until(() => localAi.machine !== null, "this computer is read");
  flushSync();
}

/** Texts the live region took, in order. */
function recordLive(): string[] {
  const live = target.querySelector('.models [role="status"]');
  if (!live) throw new Error("no live region");
  expect(live.getAttribute("aria-live")).toBe("polite");
  const seen: string[] = [];
  new MutationObserver(() => {
    const now = live.textContent?.trim() ?? "";
    if (now !== "" && seen.at(-1) !== now) seen.push(now);
  }).observe(live, { childList: true, characterData: true, subtree: true });
  return seen;
}

beforeEach(() => {
  document.body.replaceChildren();
  target = document.createElement("div");
  document.body.append(target);
  probe.machine = structuredClone(MOCK_MACHINE);
  probe.stepMs = 30;
});

afterEach(async () => {
  if (component) await unmount(component);
  component = null;
  localAi.stop();
  probe.stopAll();
  vi.restoreAllMocks();
});

describe("the list", () => {
  it("lists installed first, then with an update, then the rest by name", async () => {
    await openPage();
    const list = target.querySelector<HTMLUListElement>(".models ul");
    expect(list?.getAttribute("aria-label")).toBe("Models");
    const names = Array.from(list?.querySelectorAll(":scope > li .name") ?? []).map(
      (n) => n.textContent,
    );
    expect(names.map((n) => n?.slice(0, 20))).toEqual([
      "EmbeddingGemma 2",
      "Gemma 4 E2B",
      "EmbeddingGemma 300M",
      "Gemma 3 270M",
      "Gemma 4 E4B",
      "LFM3 1.7B",
      LONG_NAME.slice(0, 20),
      "Whisper large-v3-tur",
    ]);
  });

  it("says size, terms, abilities, languages and how it runs here (words + icon)", async () => {
    await openPage();
    expect(text(LFM)).toContain("1.1 GB");
    expect(text(LFM)).toContain("Writes text · Uses tools");
    expect(text(LFM)).toContain("Languages: English, Arabic, Chinese, French, and 9 more");
    const fit = row(GEMMA_E4B).querySelector(".fit");
    expect(fit?.textContent?.trim()).toBe("Too big for this computer");
    expect(fit?.querySelector("svg")?.getAttribute("aria-hidden")).toBe("true");
    expect(row(EMBED).querySelector(".fit")?.textContent?.trim()).toBe("Runs well here");
  });

  it("opens the terms in the browser, never in the app", async () => {
    await openPage();
    const link = row(LFM).querySelector<HTMLAnchorElement>("a.license");
    expect(link?.textContent).toBe("LFM Open License v1.0");
    expect(link?.getAttribute("aria-label")).toContain("LFM3 1.7B");
    const click = new MouseEvent("click", { bubbles: true, cancelable: true });
    link?.dispatchEvent(click);
    expect(click.defaultPrevented).toBe(true);
    await until(() => mock.probe.web.includes("https://www.liquid.ai/lfm-license"), "opened");
  });

  it("wraps a long name instead of overflowing", async () => {
    await openPage();
    const name = row(LONG).querySelector(".name");
    expect(name?.textContent).toContain(LONG_NAME);
    expect(name?.classList.contains("name")).toBe(true);
  });

  it("shows format, revision, source and who may use it under Details", async () => {
    await openPage();
    const details = row(LFM).querySelector("details");
    const summary = details?.querySelector("summary");
    expect(summary?.getAttribute("aria-label")).toBe("Details of LFM3 1.7B");
    expect(details?.open).toBe(false);
    summary?.click();
    expect(details?.textContent).toContain("Q4_K_M");
    expect(details?.textContent).toContain("0000000");
    expect(details?.textContent).toContain("LiquidAI/LFM3-1.7B-GGUF");
    expect(details?.textContent).toContain("Free for organizations under $10M annual revenue");
  });
});

describe("when Get cannot start", () => {
  it("needs a token: Get is off and the row says why", async () => {
    await openPage();
    const get = action(GATED, "get");
    expect(get?.disabled).toBe(true);
    expect(get?.getAttribute("aria-label")).toBe("Get Gemma 3 270M (241 MB)");
    const reason = document.getElementById(get?.getAttribute("aria-describedby") ?? "");
    expect(reason?.textContent).toContain("Hugging Face token");
  });

  it("not enough space: Get is off, with the sizes", async () => {
    await openPage();
    const get = action(GEMMA_E4B, "get");
    expect(get?.disabled).toBe(true);
    expect(text(GEMMA_E4B)).toContain("Not enough space: needs 5.2 GB, 4.3 GB free.");
  });

  it("storage not available: rows say so, Get is off", async () => {
    await openPage();
    const now = localAi.state;
    if (now === null) throw new Error("no state");
    localAi.state = {
      ...now,
      models: now.models.map((m) =>
        m.installedVersion === null ? { ...m, state: { kind: "storeUnavailable" } } : m,
      ),
    };
    flushSync();
    expect(action(LONG, "get")?.disabled).toBe(true);
    expect(text(LONG)).toContain("Storage not available");
    expect(action(LFM, "terms")).toBeNull();
  });
});

describe("getting a model", () => {
  it("Get -> progress -> Ready; the live region says start, quarters and done only", async () => {
    await openPage();
    const live = recordLive();
    press(LONG, "get");
    await until(() => row(LONG).querySelector('[role="progressbar"]') !== null, "progress shows");
    const bar = row(LONG).querySelector('[role="progressbar"]');
    expect(bar?.getAttribute("aria-valuemin")).toBe("0");
    expect(bar?.getAttribute("aria-valuemax")).toBe("100");
    expect(Number(bar?.getAttribute("aria-valuenow"))).toBeGreaterThanOrEqual(0);
    expect(bar?.getAttribute("aria-label")).toContain(LONG_NAME);
    expect(action(LONG, "cancel")?.getAttribute("aria-label")).toContain(LONG_NAME);
    expect(text(LONG)).toMatch(/MB of 157 MB/);
    await until(() => text(LONG).includes("Ready"), "the model is ready");
    const name = localAi.modelOf(LONG)?.name ?? "";
    // one message per milestone, though the bar moved ten times
    expect(live.map((said) => said.replace(name, "X"))).toEqual([
      "Getting X.",
      "X: 25% done.",
      "X: 50% done.",
      "X: 75% done.",
      "X is ready.",
    ]);
  });

  it("asks to accept the terms inline before getting", async () => {
    await openPage();
    expect(action(LFM, "get")).toBeNull();
    const opener = press(LFM, "terms");
    expect(opener.getAttribute("aria-expanded")).toBe("true");
    const box = row(LFM).querySelector<HTMLInputElement>('input[type="checkbox"]');
    await until(() => document.activeElement === box, "focus on the checkbox");
    expect(action(LFM, "acceptAndGet")?.disabled).toBe(true);
    expect(text(LFM)).toContain("Terms of LFM3 1.7B: LFM Open License v1.0");
    expect(text(LFM)).toContain("Who may use it: Free for organizations");
    // Escape closes, the keyboard is back on the button that opened it
    if (!box) throw new Error("no checkbox");
    escape(box);
    expect(row(LFM).querySelector('input[type="checkbox"]')).toBeNull();
    await until(() => document.activeElement === action(LFM, "terms"), "focus back on opener");
    press(LFM, "terms");
    row(LFM).querySelector<HTMLInputElement>('input[type="checkbox"]')?.click();
    flushSync();
    expect(action(LFM, "acceptAndGet")?.disabled).toBe(false);
    press(LFM, "acceptAndGet");
    await until(() => probe.calls.includes(`get ${LFM}`), "get after accepting");
    const accept = probe.calls.indexOf(`acceptLicense ${LFM}`);
    expect(accept).toBeGreaterThanOrEqual(0);
    expect(accept).toBeLessThan(probe.calls.indexOf(`get ${LFM}`));
    await until(() => action(LFM, "cancel") !== null, "the download runs");
    expect(row(LFM).querySelector('input[type="checkbox"]')).toBeNull();
    press(LFM, "cancel");
    await until(() => action(LFM, "cancel") === null, "stopped");
  });

  it("Cancel keeps the part (Paused), Resume goes on, Delete partial drops it", async () => {
    await openPage();
    expect(text(WHISPER)).toContain("Paused at 42%");
    expect(action(WHISPER, "get")?.getAttribute("aria-label")).toBe(
      "Resume getting Whisper large-v3-turbo",
    );
    press(WHISPER, "get");
    await until(() => action(WHISPER, "cancel") !== null, "running again");
    press(WHISPER, "cancel");
    await until(() => text(WHISPER).includes("Paused at"), "paused again");
    expect(probe.calls.filter((c) => c === `cancel ${WHISPER}`)).toHaveLength(1);
    press(WHISPER, "deletePart");
    await until(() => action(WHISPER, "get")?.textContent?.trim() === "Get 574 MB", "gone");
    expect(probe.calls).toContain(`deletePart ${WHISPER}`);
    await until(() => document.activeElement === action(WHISPER, "get"), "focus on Get");
  });

  it("a failure says why (alert) and can be tried again", async () => {
    await openPage();
    const live = recordLive();
    const alert = row(FLAKY).querySelector('[role="alert"]');
    expect(alert?.textContent).toBe("No connection to the download site.");
    const again = action(FLAKY, "get");
    expect(again?.getAttribute("aria-label")).toBe("Try getting EmbeddingGemma 300M again");
    press(FLAKY, "get");
    await until(() => probe.calls.includes(`get ${FLAKY}`), "tried again");
    await until(
      () => text(FLAKY).includes("The file wasn't the official one and was discarded."),
      "failed its check",
    );
    expect(live.at(-1)).toBe("Couldn't get EmbeddingGemma 300M.");
  });

  it("updates a model that has a newer version", async () => {
    await openPage();
    expect(text(GEMMA_E2B)).toContain("Update available");
    const update = action(GEMMA_E2B, "update");
    expect(update?.textContent?.trim()).toBe("Update 3.4 GB");
    expect(update?.getAttribute("aria-label")).toBe("Update Gemma 4 E2B (3.4 GB)");
    expect(action(GEMMA_E2B, "remove")).not.toBeNull();
    press(GEMMA_E2B, "update");
    await until(() => probe.calls.includes(`update ${GEMMA_E2B}`), "update asked");
    await until(() => action(GEMMA_E2B, "cancel") !== null, "the update runs");
    press(GEMMA_E2B, "cancel");
    await until(() => action(GEMMA_E2B, "cancel") === null, "stopped");
  });
});

describe("removing a model", () => {
  it("confirms inline, naming the size and the jobs that lose it; focus follows", async () => {
    await openPage();
    expect(text(EMBED)).toContain("Ready");
    const opener = press(EMBED, "remove");
    expect(opener.getAttribute("aria-label")).toBe("Remove EmbeddingGemma 2");
    expect(text(EMBED)).toContain(
      "Remove EmbeddingGemma 2? It frees 310 MB. Search by meaning will have no model.",
    );
    await until(() => document.activeElement?.closest(".panel") != null, "focus in the confirm");
    const cancel = document.activeElement;
    expect(cancel?.textContent?.trim()).toBe("Cancel");
    if (!cancel) throw new Error("no focus");
    escape(cancel);
    expect(action(EMBED, "confirmRemove")).toBeNull();
    await until(() => document.activeElement === action(EMBED, "remove"), "focus back on Remove");
    expect(probe.calls).not.toContain(`remove ${EMBED}`);
    press(EMBED, "remove");
    press(EMBED, "confirmRemove");
    await until(() => probe.calls.includes(`remove ${EMBED}`), "removed");
    await until(() => action(EMBED, "get") !== null, "Get offered again");
    await until(() => document.activeElement === action(EMBED, "get"), "focus on Get");
    expect(localAi.assigned("semanticIndex")).toBeNull();
  });
});

describe("milestones", () => {
  const total = 1000;
  const at = (done: number): InstallState => ({ kind: "downloading", done, total });

  it("start, each quarter, done and failure; nothing in between", () => {
    expect(milestoneOf({ kind: "notInstalled" }, { kind: "queued" })).toBe("started");
    expect(milestoneOf({ kind: "paused", done: 400, total }, { kind: "queued" })).toBe("started");
    expect(milestoneOf({ kind: "queued" }, at(0))).toBeNull();
    expect(milestoneOf(at(200), at(300))).toBe(25);
    expect(milestoneOf(at(300), at(400))).toBeNull();
    expect(milestoneOf(at(400), at(500))).toBe(50);
    expect(milestoneOf(at(700), at(800))).toBe(75);
    expect(milestoneOf(at(900), { kind: "verifying", done: total, total })).toBeNull();
    expect(
      milestoneOf(
        { kind: "verifying", done: total, total },
        { kind: "installed", catalogVersion: 1 },
      ),
    ).toBe("done");
    expect(milestoneOf(at(100), { kind: "failed", error: { kind: "offline" } })).toBe("failed");
    expect(
      milestoneOf(
        { kind: "failed", error: { kind: "offline" } },
        { kind: "failed", error: { kind: "offline" } },
      ),
    ).toBeNull();
    expect(
      milestoneOf({ kind: "notInstalled" }, { kind: "installed", catalogVersion: 1 }),
    ).toBeNull();
  });
});

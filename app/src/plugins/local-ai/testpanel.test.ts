// @vitest-environment jsdom
// "Test model" against the mock backend: the Test button of an installed model, the panel through each phase (engine,
// loading, running, done | failed | cancelled), one test at a time, Cancel, Try again, Close + focus, the failure words.
import { flushSync, mount, unmount } from "svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { ApiError } from "../../api";
import type { IpcError } from "../../api/generated/IpcError";

vi.mock("../../api", async () => {
  const { createMockApi } = await import("../../api/mock");
  return { api: createMockApi() };
});

const { api } = await import("../../api");
const { MOCK_ENGINE_PIN, MOCK_MACHINE } = await import("../../api/mockLocalAi");
const { localAi } = await import("./store.svelte");
const { testErrorText } = await import("./words");
const { default: LocalAiSection } = await import("./LocalAiSection.svelte");

const mock = api as unknown as ReturnType<typeof import("../../api/mock").createMockApi>;
const probe = mock.probe.localAi;

const EMBED = "embeddinggemma-2-q8_0";
const CHAT = "gemma-4-e2b-it-q4_0";
const NOT_HERE = "lfm3-1.7b-q4_k_m";
/** A later llama.cpp worker: the installed EmbeddingGemma 2 runs (the pinned one cannot). */
const NEWER = { engine: "llama.cpp", build: "b11960" } as const;

let target: HTMLElement;
let component: Record<string, unknown> | null = null;

async function until(check: () => boolean, what: string): Promise<void> {
  await vi.waitFor(() => expect(check(), what).toBe(true), { timeout: 10_000, interval: 5 });
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

function panel(model: string): HTMLElement | null {
  return row(model).querySelector<HTMLElement>(".test");
}

const said = (model: string): string =>
  (panel(model)?.textContent ?? "").replace(/\s+/g, " ").trim();

/** Texts the panel's live region took, in order. */
function recordLive(model: string): string[] {
  const live = panel(model)?.querySelector('[role="status"]');
  if (!live) throw new Error("no live region");
  expect(live.getAttribute("aria-live")).toBe("polite");
  const seen: string[] = [live.textContent?.trim() ?? ""];
  new MutationObserver(() => {
    const now = (live.textContent ?? "").replace(/\s+/g, " ").trim();
    if (now !== "" && seen.at(-1) !== now) seen.push(now);
  }).observe(live, { childList: true, characterData: true, subtree: true });
  return seen;
}

async function openPage(): Promise<void> {
  component = mount(LocalAiSection, { target });
  flushSync();
  await until(() => target.querySelector("li[data-model]") !== null, "the models card shows");
  flushSync();
}

beforeEach(() => {
  document.body.replaceChildren();
  target = document.createElement("div");
  document.body.append(target);
  probe.machine = structuredClone(MOCK_MACHINE);
  probe.stepMs = 60;
  probe.engineInstalled = false;
  probe.failTest = null;
  probe.engine = NEWER;
});

afterEach(async () => {
  if (component) await unmount(component);
  component = null;
  localAi.stop();
  probe.stopAll();
  vi.restoreAllMocks();
});

describe("the Test button", () => {
  it("is on installed models only, with a text label", async () => {
    await openPage();
    const button = action(EMBED, "test");
    expect(button?.textContent?.trim()).toBe("Test");
    expect(button?.getAttribute("aria-label")).toBe("Test EmbeddingGemma 2 on this computer");
    expect(action(CHAT, "test")).not.toBeNull();
    expect(action(NOT_HERE, "test")).toBeNull();
    expect(panel(EMBED)).toBeNull();
  });

  it("is not on an installed model the AI engine cannot run", async () => {
    probe.engine = { ...MOCK_ENGINE_PIN };
    await openPage();
    expect(action(EMBED, "test")).toBeNull();
    expect(action(CHAT, "test")).not.toBeNull();
  });

  it("is off on every model while one test runs", async () => {
    await openPage();
    probe.engineInstalled = true;
    press(CHAT, "test");
    await until(() => probe.calls.includes(`testModel ${CHAT}`), "test asked");
    expect(action(EMBED, "test")?.disabled).toBe(true);
    expect(action(CHAT, "test")?.disabled).toBe(true);
    await until(() => said(CHAT).includes("Works on this computer"), "test done");
    expect(action(EMBED, "test")?.disabled).toBe(false);
  });
});

describe("the panel", () => {
  it("gets the engine, loads, runs, then says what it measured (embedding)", async () => {
    await openPage();
    press(EMBED, "test");
    const live = recordLive(EMBED);
    // the Test button is off: the keyboard goes to Cancel
    await until(() => document.activeElement === action(EMBED, "cancelTest"), "focus on Cancel");
    await until(() => said(EMBED).includes("Getting the AI engine (first time only)"), "engine");
    await until(() => /MB of 50 MB/.test(said(EMBED)), "engine sizes");
    const bar = panel(EMBED)?.querySelector("progress");
    expect(bar?.getAttribute("aria-label")).toBe("Getting the AI engine (first time only)");
    expect(action(EMBED, "cancelTest")?.getAttribute("aria-label")).toBe(
      "Stop the test of EmbeddingGemma 2",
    );
    await until(() => said(EMBED).includes("Loading the model"), "loading");
    await until(() => /Loading the model\s*\d+%/.test(said(EMBED)), "loading percent");
    expect(panel(EMBED)?.querySelector("progress")?.getAttribute("aria-label")).toBe(
      "Loading the model",
    );
    await until(() => said(EMBED).includes("Works on this computer"), "done");
    const text = said(EMBED);
    expect(text).toContain("Runs on the graphics card");
    expect(text).toContain("Loaded in 0.6 seconds");
    expect(text).toContain("Ready to find text by meaning");
    expect(text).toMatch(/Uses about \d+ MB of memory/);
    expect(panel(EMBED)?.querySelector("progress")).toBeNull();
    expect(live).toContain("Getting the AI engine (first time only)");
    expect(live).toContain("Loading the model");
    expect(live).toContain("Running a short test");
    expect(live.at(-1)).toBe("Works on this computer");
    // Close: panel gone, the keyboard back on Test
    await until(() => document.activeElement === action(EMBED, "closeTest"), "focus on Close");
    press(EMBED, "closeTest");
    expect(panel(EMBED)).toBeNull();
    await until(() => document.activeElement === action(EMBED, "test"), "focus on Test");
  });

  it("shows speed and the answer of a chat model, on the processor too", async () => {
    await openPage();
    probe.engineInstalled = true;
    press(CHAT, "test");
    await until(() => said(CHAT).includes("Works on this computer"), "done");
    const text = said(CHAT);
    expect(text).not.toContain("Getting the AI engine");
    expect(text).toContain("Loaded in 1.8 seconds");
    expect(text).toContain("8.6 tokens a second");
    expect(panel(CHAT)?.querySelector("blockquote")?.textContent?.trim()).toBe(
      "Hello! Nice to meet you.",
    );
    // the device as the result says
    const now = localAi.modelTest;
    if (now === null || now.step.kind !== "done") throw new Error("not done");
    localAi.modelTest = {
      ...now,
      step: { ...now.step, result: { ...now.step.result, gpu: false, memoryBytes: null } },
    };
    flushSync();
    expect(said(CHAT)).toContain("Runs on the processor");
    expect(said(CHAT)).not.toContain("of memory");
  });

  it("Cancel stops the test: 'Test stopped.' + Close", async () => {
    await openPage();
    press(EMBED, "test");
    await until(() => said(EMBED).includes("Getting the AI engine"), "engine");
    press(EMBED, "cancelTest");
    await until(() => probe.calls.includes("cancelTest"), "cancel asked");
    await until(() => said(EMBED).includes("Test stopped."), "stopped");
    expect(action(EMBED, "cancelTest")).toBeNull();
    expect(action(EMBED, "test")?.disabled).toBe(false);
    press(EMBED, "closeTest");
    expect(panel(EMBED)).toBeNull();
  });

  it("a failure says why in plain words; Try again tests again", async () => {
    await openPage();
    probe.engineInstalled = true;
    probe.failTest = { kind: "unavailable", subject: "engine", detail: "outOfMemory" };
    press(EMBED, "test");
    await until(() => said(EMBED).includes("Not enough free memory"), "failed");
    expect(said(EMBED)).toContain("smaller model");
    const again = action(EMBED, "retryTest");
    expect(again?.textContent?.trim()).toBe("Try again");
    expect(again?.getAttribute("aria-label")).toBe("Test EmbeddingGemma 2 again");
    const asked = (): number => probe.calls.filter((c) => c === `testModel ${EMBED}`).length;
    const before = asked();
    press(EMBED, "retryTest");
    await until(() => asked() === before + 1, "tested again");
    await until(() => said(EMBED).includes("Works on this computer"), "works now");
  });

  it("a refusal shows in the panel too", async () => {
    await openPage();
    // another window started a test a moment ago: no news of it here yet
    probe.engineInstalled = true;
    probe.stepMs = 5_000;
    await mock.pluginCall("local-ai.testModel", { model: CHAT });
    press(EMBED, "test");
    await until(() => said(EMBED).includes("Another test is running"), "refused");
    expect(action(EMBED, "retryTest")).not.toBeNull();
  });
});

describe("failure words", () => {
  const engine = (kind: IpcError["kind"], detail: string): ApiError => ({
    kind: "backend",
    command: "plugin_call",
    error: { kind, subject: "engine", detail },
  });

  it("maps each engine failure to its own text", () => {
    expect(testErrorText(engine("unavailable", "outOfMemory"))).toMatch(/Not enough free memory/);
    expect(testErrorText(engine("unavailable", "notAvailable"))).toMatch(/isn't available/);
    expect(testErrorText(engine("unavailable", "crashed"))).toMatch(/stopped unexpectedly/);
    // session lost (worker gone) reads like a crash
    expect(testErrorText(engine("unavailable", "notLoaded"))).toMatch(/stopped unexpectedly/);
    expect(testErrorText(engine("denied", "blocked"))).toMatch(/Security software/);
    const newer = testErrorText(engine("unsupported", "engineTooOld"));
    expect(newer).toMatch(/newer version of the AI engine/);
    expect(testErrorText(engine("unsupported", "unsupported"))).toBe(newer);
    expect(testErrorText(engine("invalidData", "corrupt"))).toMatch(/damaged.*get it again/);
  });

  it("refusals have their words; anything else takes the general error text", () => {
    const refusal = (kind: IpcError["kind"], detail: string): ApiError => ({
      kind: "backend",
      command: "plugin_call",
      error: { kind, subject: null, detail },
    });
    expect(testErrorText(refusal("conflict", "testRunning"))).toMatch(/Another test is running/);
    expect(testErrorText(refusal("unsupported", "noTest"))).toMatch(/can't be tested/);
    expect(testErrorText(refusal("conflict", "notInstalled"))).toBe(
      "This model isn't on this computer.",
    );
    const general = testErrorText(engine("internal", "badPin"));
    expect(general).not.toBe("");
    expect(testErrorText({ kind: "window", message: "gone" })).not.toBe("");
  });
});

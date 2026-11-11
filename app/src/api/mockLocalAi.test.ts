import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

// the bundled catalog itself: the mock's engine floors must match it
import CATALOG from "../../src-tauri/crates/plugins/local-ai/src/catalog.json";
import type { InstallState } from "./generated/InstallState";
import type { LocalAiState } from "./generated/LocalAiState";
import type { ModelTest } from "./generated/ModelTest";
import {
  createMockLocalAi,
  MOCK_CATALOG,
  MOCK_DEFAULT_STORE,
  MOCK_MACHINE,
  MOCK_MIN_ENGINE,
  MOCK_PROFILE,
} from "./mockLocalAi";
import { MockPluginError } from "./mockOrganizing";
import type { AppPluginEventName } from "./types";

const EMBED = "embeddinggemma-2-q8_0";
const E2B = "gemma-4-e2b-it-q4_0";
const E4B = "gemma-4-e4b-it-q4_0";
const LFM = "lfm3-1.7b-q4_k_m";
const WHISPER = "whisper-large-v3-turbo-q5_0";
const GATED = "gemma-3-270m-it-q8_0";
const FLAKY = "embeddinggemma-300m-q4_0";
const GERMAN = "fachtext-uebersetzer-q4_0";

const PROJECT = "D:/Work/Notes";
/** A later llama.cpp worker (runs EmbeddingGemma 2 + LFM3); a speech worker (runs Whisper). */
const NEWER = { engine: "llama.cpp", build: "b11960" } as const;
const SPEECH = { engine: "whisper.cpp", build: "v1.7.0" } as const;

function setup() {
  const events: { name: AppPluginEventName; payload: unknown }[] = [];
  const mock = createMockLocalAi(
    (name, payload) => events.push({ name, payload }),
    () => [PROJECT],
  );
  const h = mock.handlers;
  const of = (state: LocalAiState, id: string): InstallState => {
    const found = state.models.find((m) => m.model === id);
    if (found === undefined) throw new Error(id);
    return found.state;
  };
  const now = (id: string): InstallState => of(h["local-ai.state"]({}), id);
  return { ...mock, h, events, now };
}

function refusal(run: () => unknown): MockPluginError {
  try {
    run();
  } catch (error) {
    if (error instanceof MockPluginError) return error;
    throw error;
  }
  throw new Error("not refused");
}

describe("local AI mock installs", () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());

  it("starts with a fixture for every state", () => {
    const { now, h } = setup();
    expect(now(EMBED)).toEqual({ kind: "installed", catalogVersion: 1 });
    expect(now(E2B)).toEqual({ kind: "updateAvailable", installedVersion: 1 });
    expect(now(WHISPER)).toMatchObject({ kind: "paused" });
    const paused = now(WHISPER);
    if (paused.kind !== "paused") throw new Error(paused.kind);
    expect(Math.round((paused.done / paused.total) * 100)).toBe(42);
    expect(now(FLAKY)).toEqual({ kind: "failed", error: { kind: "offline" } });
    for (const id of [LFM, GATED, GERMAN, E4B])
      expect(now(id), id).toEqual({ kind: "notInstalled" });
    const state = h["local-ai.state"]({});
    const blocked = Object.fromEntries(state.models.map((m) => [m.model, m.blocked]));
    expect(blocked[LFM]).toBe("license");
    expect(blocked[GATED]).toBe("token");
    expect(state.assignments.find((a) => a.task === "semanticIndex")?.model).toBe(EMBED);
    const german = MOCK_CATALOG.models.find((m) => m.id === GERMAN);
    expect(german?.name.length).toBeGreaterThan(60);
    expect(german?.languages).toContain("ja");
  });

  it("gets a model: queued -> downloading by steps -> verifying -> installed, with events", () => {
    const { h, now, events } = setup();
    h["local-ai.get"]({ model: GERMAN });
    expect(now(GERMAN)).toEqual({ kind: "queued" });
    vi.advanceTimersByTime(400 * 2);
    expect(now(GERMAN)).toMatchObject({ kind: "downloading" });
    vi.advanceTimersByTime(400 * 9);
    expect(now(GERMAN)).toMatchObject({ kind: "verifying" });
    vi.advanceTimersByTime(400);
    expect(now(GERMAN)).toEqual({ kind: "installed", catalogVersion: 1 });
    expect(events.some((e) => e.name === "local-ai:progress")).toBe(true);
    expect(events.at(-1)?.name).toBe("local-ai:state");
  });

  it("cancel keeps the part (paused); get resumes; delete part drops it", () => {
    const { h, now, probe } = setup();
    h["local-ai.get"]({ model: GERMAN });
    vi.advanceTimersByTime(400 * 4);
    h["local-ai.cancel"]({ model: GERMAN });
    const paused = now(GERMAN);
    expect(paused).toMatchObject({ kind: "paused" });
    vi.advanceTimersByTime(400 * 20);
    expect(now(GERMAN)).toEqual(paused);
    h["local-ai.deletePart"]({ model: GERMAN });
    expect(now(GERMAN)).toEqual({ kind: "notInstalled" });
    probe.engine = SPEECH;
    h["local-ai.get"]({ model: WHISPER });
    vi.advanceTimersByTime(400 * 12);
    expect(now(WHISPER)).toEqual({ kind: "installed", catalogVersion: 1 });
  });

  it("refuses a gated model without a token, gets it with one (never shown)", () => {
    const { h, now } = setup();
    const error = refusal(() => h["local-ai.get"]({ model: GATED }));
    expect([error.kind, error.detail]).toEqual(["denied", "token"]);
    expect(refusal(() => h["local-ai.setToken"]({ token: "  " })).kind).toBe("invalidData");
    const after = h["local-ai.setToken"]({ token: "hf_secret" });
    expect(after.tokenSet).toBe(true);
    expect(JSON.stringify(after)).not.toContain("hf_secret");
    h["local-ai.get"]({ model: GATED });
    expect(now(GATED)).toEqual({ kind: "queued" });
    expect(h["local-ai.clearToken"]({}).tokenSet).toBe(false);
  });

  it("asks for the terms before the first byte, once", () => {
    const { h, now, probe } = setup();
    probe.engine = NEWER;
    const error = refusal(() => h["local-ai.get"]({ model: LFM }));
    expect([error.kind, error.detail]).toEqual(["denied", "license"]);
    expect(now(LFM)).toEqual({ kind: "notInstalled" });
    const after = h["local-ai.acceptLicense"]({ model: LFM });
    expect(after.acceptedLicenses).toEqual(["https://www.liquid.ai/lfm-license"]);
    expect(after.models.find((m) => m.model === LFM)?.blocked).toBeNull();
    h["local-ai.get"]({ model: LFM });
    expect(now(LFM)).toEqual({ kind: "queued" });
    const terms = MOCK_CATALOG.models.filter((m) => m.license.needsAcceptance);
    expect(terms.every((m) => m.license.revision !== null)).toBe(true);
  });

  it("refuses a token longer than 1 KiB", () => {
    const { h } = setup();
    const long = refusal(() => h["local-ai.setToken"]({ token: "x".repeat(1025) }));
    expect([long.kind, long.detail]).toEqual(["invalidData", "tooLong"]);
    expect(h["local-ai.setToken"]({ token: "x".repeat(1024) }).tokenSet).toBe(true);
  });

  it("fails what does not fit the disk, and a retry that fails its check", () => {
    const { h, now } = setup();
    h["local-ai.get"]({ model: E4B });
    expect(now(E4B)).toMatchObject({ kind: "failed", error: { kind: "noSpace" } });
    h["local-ai.get"]({ model: FLAKY });
    vi.advanceTimersByTime(400 * 12);
    expect(now(FLAKY)).toEqual({ kind: "failed", error: { kind: "integrity" } });
  });

  it("updates beside the old version; remove unassigns; assign checks install + fit", () => {
    const { h, now, probe } = setup();
    probe.engine = NEWER;
    h["local-ai.update"]({ model: E2B });
    const during = h["local-ai.state"]({}).models.find((m) => m.model === E2B);
    expect(during?.installedVersion).toBe(1);
    vi.advanceTimersByTime(400 * 12);
    expect(now(E2B)).toEqual({ kind: "installed", catalogVersion: 2 });
    expect(refusal(() => h["local-ai.update"]({ model: E2B })).detail).toBe("noUpdate");

    expect(refusal(() => h["local-ai.assign"]({ task: "correction", model: EMBED })).detail).toBe(
      "incompatible",
    );
    expect(refusal(() => h["local-ai.assign"]({ task: "correction", model: LFM })).detail).toBe(
      "notInstalled",
    );
    h["local-ai.assign"]({ task: "assistant", model: E2B });
    const after = h["local-ai.remove"]({ model: E2B });
    expect(after.assignments.every((a) => a.model === null || a.model === EMBED)).toBe(true);
    expect(now(E2B)).toEqual({ kind: "notInstalled" });
    h["local-ai.get"]({ model: GERMAN });
    expect(refusal(() => h["local-ai.remove"]({ model: GERMAN })).detail).toBe("downloading");
    h["local-ai.assign"]({ task: "semanticIndex", model: null });
    expect(
      h["local-ai.state"]({}).assignments.find((a) => a.task === "semanticIndex")?.model,
    ).toBeNull();
  });
});

describe("local AI mock engine", () => {
  it("knows the engine floors of the bundled catalog", () => {
    const floors = Object.fromEntries(CATALOG.models.map((m) => [m.id, m.minEngine]));
    const mirrored = Object.fromEntries(Object.keys(floors).map((id) => [id, MOCK_MIN_ENGINE[id]]));
    expect(mirrored).toEqual(floors);
  });

  it("offers only what the pinned engine runs; refuses Get, assign, Test of the rest", () => {
    const { h, now } = setup();
    const needs = () =>
      Object.fromEntries(h["local-ai.state"]({}).models.map((m) => [m.model, m.engineNeed]));
    expect(needs()).toMatchObject({
      [E2B]: null,
      [E4B]: null,
      [EMBED]: "newer",
      [LFM]: "newer",
      [WHISPER]: "missing",
      [GERMAN]: null,
    });
    h["local-ai.acceptLicense"]({ model: LFM });
    const get = refusal(() => h["local-ai.get"]({ model: LFM }));
    expect([get.kind, get.subject, get.detail]).toEqual(["unsupported", LFM, "newerEngine"]);
    expect(refusal(() => h["local-ai.get"]({ model: WHISPER })).detail).toBe("missingEngine");
    expect(now(LFM)).toEqual({ kind: "notInstalled" });
    // installed earlier: listed, assignment kept, never assigned again nor tested
    expect(now(EMBED)).toEqual({ kind: "installed", catalogVersion: 1 });
    const state = h["local-ai.state"]({});
    expect(state.assignments.find((a) => a.task === "semanticIndex")?.model).toBe(EMBED);
    const assign = refusal(() => h["local-ai.assign"]({ task: "semanticIndex", model: EMBED }));
    expect(assign.detail).toBe("newerEngine");
    const test = refusal(() => h["local-ai.testModel"]({ model: EMBED }));
    expect([test.kind, test.subject, test.detail]).toEqual([
      "unsupported",
      "engine",
      "engineTooOld",
    ]);
    // a later pin: the same models run, nothing else changed
    const { h: later, probe } = setup();
    probe.engine = NEWER;
    const after = Object.fromEntries(
      later["local-ai.state"]({}).models.map((m) => [m.model, m.engineNeed]),
    );
    expect(after).toMatchObject({ [EMBED]: null, [LFM]: null, [WHISPER]: "missing" });
    probe.engine = null;
    expect(later["local-ai.state"]({}).models.every((m) => m.engineNeed === "missing")).toBe(true);
  });
});

describe("local AI mock store", () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());

  it("starts on the default store with its bytes", () => {
    const { h, dataBytes } = setup();
    const { store, storeMove } = h["local-ai.state"]({});
    expect(store).toMatchObject({
      path: MOCK_DEFAULT_STORE,
      isDefault: true,
      available: true,
      unavailableReason: null,
    });
    expect(store.usedBytes).toBe(309_855_456 + 3_398_849_248);
    expect(store.partialBytes).toBeGreaterThan(0);
    expect(store.freeBytes).toBeGreaterThan(0);
    expect(storeMove).toBeNull();
    expect(dataBytes()).toBe(store.usedBytes + store.partialBytes);
  });

  it("refuses relative paths, project folders, the store itself and a change during a download", () => {
    const { h } = setup();
    const set = (path: string) => () => h["local-ai.setStore"]({ path, mode: "keep" });
    expect(refusal(set("models")).detail).toBe("notAbsolute");
    expect(refusal(set("D:/Work/Notes/models")).detail).toBe("insideProject");
    expect(refusal(set("D:/work")).detail).toBe("insideProject");
    expect(refusal(set(`${MOCK_DEFAULT_STORE}/sub`)).detail).toBe("insideStore");
    expect(refusal(set(`${MOCK_PROFILE}/plugins/notes`)).detail).toBe("insideAppData");
    h["local-ai.get"]({ model: GERMAN });
    const busy = refusal(set("E:/Models"));
    expect([busy.kind, busy.detail]).toEqual(["conflict", "downloading"]);
  });

  it("asks move or keep; keep starts empty and the old folder keeps its models", () => {
    const { h, now } = setup();
    expect(refusal(() => h["local-ai.setStore"]({ path: "E:/Models" })).detail).toBe(
      "modeRequired",
    );
    const kept = h["local-ai.setStore"]({ path: "E:/Models", mode: "keep" });
    expect(kept.store).toMatchObject({ path: "E:/Models", isDefault: false, usedBytes: 0 });
    expect(now(EMBED)).toEqual({ kind: "notInstalled" });
    h["local-ai.setStore"]({ path: null });
    expect(now(EMBED)).toEqual({ kind: "installed", catalogVersion: 1 });
  });

  it("moves by steps with progress events; installs wait; the models follow", () => {
    const { h, now, events } = setup();
    const moving = h["local-ai.setStore"]({ path: "E:/Models", mode: "move" });
    expect(moving.storeMove).toMatchObject({ kind: "moving", to: "E:/Models", done: 0 });
    expect(moving.store.isDefault).toBe(true);
    expect(refusal(() => h["local-ai.get"]({ model: GERMAN })).detail).toBe("moving");
    vi.advanceTimersByTime(400 * 3);
    expect(events.some((e) => e.name === "local-ai:move")).toBe(true);
    vi.advanceTimersByTime(400 * 10);
    const after = h["local-ai.state"]({});
    expect(after.storeMove).toBeNull();
    expect(after.store.path).toBe("E:/Models");
    expect(now(EMBED)).toEqual({ kind: "installed", catalogVersion: 1 });
    expect(after.assignments.find((a) => a.task === "semanticIndex")?.model).toBe(EMBED);
    // the old folder no longer holds them
    h["local-ai.setStore"]({ path: null, mode: "keep" });
    expect(now(EMBED)).toEqual({ kind: "notInstalled" });
  });

  it("a failed or cancelled move leaves the old store in use", () => {
    const { h, now, probe } = setup();
    probe.failMove = { kind: "integrity" };
    h["local-ai.setStore"]({ path: "E:/Models", mode: "move" });
    vi.advanceTimersByTime(400 * 12);
    const failed = h["local-ai.state"]({});
    expect(failed.storeMove).toEqual({
      kind: "failed",
      to: "E:/Models",
      error: { kind: "integrity" },
    });
    expect(failed.store.isDefault).toBe(true);
    h["local-ai.setStore"]({ path: "E:/Models", mode: "move" });
    vi.advanceTimersByTime(400 * 2);
    const cancelled = h["local-ai.cancelMove"]({});
    expect(cancelled.storeMove).toBeNull();
    expect(cancelled.store.isDefault).toBe(true);
    expect(now(EMBED)).toEqual({ kind: "installed", catalogVersion: 1 });
  });

  it("a gone store is unavailable, not empty; reveal shows the store folder", () => {
    const { h, probe } = setup();
    h["local-ai.revealStore"]({});
    expect(probe.calls).toContain(`revealStore ${MOCK_DEFAULT_STORE}`);
    const gone = h["local-ai.setStore"]({ path: "F:/Unplugged drive", mode: "keep" });
    expect(gone.store).toMatchObject({
      available: false,
      unavailableReason: "missing",
      freeBytes: null,
    });
    expect(gone.models.every((m) => m.state.kind === "storeUnavailable")).toBe(true);
    expect(refusal(() => h["local-ai.get"]({ model: GERMAN })).kind).toBe("unavailable");
    expect(refusal(() => h["local-ai.revealStore"]({})).kind).toBe("unavailable");
  });

  it("a store a project opened since overlaps is unavailable with that reason", () => {
    const projects: string[] = [];
    const h = createMockLocalAi(
      () => {},
      () => projects,
    ).handlers;
    h["local-ai.setStore"]({ path: "E:/Models", mode: "keep" });
    projects.push("E:/");
    const state = h["local-ai.state"]({});
    expect(state.store).toMatchObject({ available: false, unavailableReason: "insideProject" });
    expect(state.models.every((m) => m.state.kind === "storeUnavailable")).toBe(true);
  });
});

describe("local AI mock machine", () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());

  it("answers the profile and a fit per model: runs well, slow, not recommended", () => {
    const { h } = setup();
    const info = h["local-ai.machine"]({});
    expect(info.profile).toEqual(MOCK_MACHINE);
    expect(Object.keys(info.fits).sort()).toEqual(MOCK_CATALOG.models.map((m) => m.id).sort());
    expect(info.fits[EMBED]).toBe("runsWell");
    expect(info.fits[WHISPER]).toBe("runsWell");
    expect(info.fits[E2B]).toBe("slowCpuOnly");
    expect(info.fits[LFM]).toBe("slowCpuOnly");
    expect(info.fits[E4B]).toBe("notRecommended");
    expect(info.budget).toBeNull();
  });

  it("warns when a second big model is assigned, and stops when it is unassigned", () => {
    const { h, probe } = setup();
    probe.engine = NEWER;
    h["local-ai.acceptLicense"]({ model: LFM });
    h["local-ai.get"]({ model: LFM });
    vi.advanceTimersByTime(400 * 12);
    h["local-ai.assign"]({ task: "assistant", model: LFM });
    const ramRec = (id: string) => MOCK_CATALOG.models.find((m) => m.id === id)?.needs.ramRec ?? 0;
    const warned = h["local-ai.machine"]({}).budget;
    expect(warned).toEqual({
      needed: ramRec(EMBED) + ramRec(E2B) + ramRec(LFM),
      available: 2 * 1_073_741_824 + Math.floor((6 * 1_073_741_824) / 5) * 3,
    });
    if (warned === null) throw new Error("no warning");
    expect(warned.needed).toBeGreaterThan(warned.available);
    h["local-ai.assign"]({ task: "assistant", model: null });
    expect(h["local-ai.machine"]({}).budget).toBeNull();
  });

  it("counts a model assigned to two tasks once; unknown memory gives unknown fits, no warning", () => {
    const { h, probe } = setup();
    h["local-ai.assign"]({ task: "translation", model: E2B });
    expect(h["local-ai.machine"]({}).budget).toBeNull();
    probe.machine = { ...MOCK_MACHINE, ramTotal: null, gpus: [] };
    const info = h["local-ai.machine"]({});
    expect(new Set(Object.values(info.fits))).toEqual(new Set(["unknown"]));
    expect(info.budget).toBeNull();
  });

  it("unified memory runs well what the RAM covers", () => {
    const { h, probe } = setup();
    probe.machine = {
      ...MOCK_MACHINE,
      ramTotal: 16 * 1_073_741_824,
      gpus: [
        { name: "Apple M2", dedicatedVram: null, shared: null, unified: true, software: false },
      ],
      os: "macos",
    };
    expect(new Set(Object.values(h["local-ai.machine"]({}).fits))).toEqual(new Set(["runsWell"]));
  });

  it("keeps models on the processor when asked: told to the windows, advice without graphics memory", () => {
    const { h, events, probe } = setup();
    expect(h["local-ai.state"]({}).cpuOnly).toBe(false);
    events.length = 0;
    expect(h["local-ai.setCpuOnly"]({ cpuOnly: true }).cpuOnly).toBe(true);
    expect(probe.calls).toContain("setCpuOnly true");
    expect(events.at(-1)).toMatchObject({ name: "local-ai:state", payload: { cpuOnly: true } });
    const info = h["local-ai.machine"]({});
    // the cards are still this computer's; the advice no longer counts them
    expect(info.profile).toEqual(MOCK_MACHINE);
    expect(info.fits[EMBED]).toBe("slowCpuOnly");
    expect(info.fits[WHISPER]).toBe("slowCpuOnly");
    expect(info.fits[E4B]).toBe("notRecommended");
    const ramRec = (id: string) => MOCK_CATALOG.models.find((m) => m.id === id)?.needs.ramRec ?? 0;
    expect(info.budget).toEqual({
      needed: ramRec(EMBED) + ramRec(E2B),
      available: Math.floor((6 * 1_073_741_824) / 5) * 3,
    });
    probe.machine = {
      ...MOCK_MACHINE,
      ramTotal: 16 * 1_073_741_824,
      gpus: [
        { name: "Apple M2", dedicatedVram: null, shared: null, unified: true, software: false },
      ],
    };
    expect(new Set(Object.values(h["local-ai.machine"]({}).fits))).toEqual(
      new Set(["slowCpuOnly"]),
    );
    probe.machine = structuredClone(MOCK_MACHINE);
    expect(h["local-ai.setCpuOnly"]({ cpuOnly: false }).cpuOnly).toBe(false);
    const back = h["local-ai.machine"]({});
    expect(back.fits[EMBED]).toBe("runsWell");
    expect(back.budget).toBeNull();
  });
});

describe("local AI mock model test", () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());

  function isTest(payload: unknown): payload is ModelTest {
    return (
      typeof payload === "object" && payload !== null && "model" in payload && "phase" in payload
    );
  }

  /** Test events so far, in order. */
  function tests(events: { name: AppPluginEventName; payload: unknown }[]): ModelTest[] {
    return events.flatMap((e) =>
      e.name === "local-ai:test" && isTest(e.payload) ? [e.payload] : [],
    );
  }

  function kinds(events: { name: AppPluginEventName; payload: unknown }[]): string[] {
    return tests(events)
      .map((t) => t.phase.kind)
      .filter((k, i, all) => k !== all[i - 1]);
  }

  it("gets the engine on the first test, loads, runs, and ends with plausible numbers", () => {
    const { h, events, probe } = setup();
    probe.engine = NEWER;
    expect(h["local-ai.testModel"]({ model: E2B })).toBeNull();
    vi.advanceTimersByTime(400 * 20);
    expect(kinds(events)).toEqual(["engine", "loading", "running", "done"]);
    const last = tests(events).at(-1);
    expect(last?.model).toBe(E2B);
    if (last?.phase.kind !== "done") throw new Error(last?.phase.kind);
    expect(last.phase.result.output).toMatchObject({ kind: "chat", tokens: 12 });
    expect(last.phase.result.gpu).toBe(true);
    expect(probe.engineInstalled).toBe(true);
    events.length = 0;
    h["local-ai.testModel"]({ model: EMBED });
    vi.advanceTimersByTime(400 * 20);
    expect(kinds(events)).toEqual(["loading", "running", "done"]);
    const embed = tests(events).at(-1)?.phase;
    if (embed?.kind !== "done") throw new Error(embed?.kind);
    expect(embed.result.output).toEqual({ kind: "embed", dims: 768 });
  });

  it("runs one test at a time; cancel ends it; refuses what cannot be tested", () => {
    const { h, events, probe } = setup();
    h["local-ai.testModel"]({ model: E2B });
    vi.advanceTimersByTime(400 * 2);
    expect(refusal(() => h["local-ai.testModel"]({ model: EMBED })).detail).toBe("testRunning");
    h["local-ai.cancelTest"]({});
    expect(tests(events).at(-1)?.phase).toEqual({ kind: "cancelled" });
    const count = events.length;
    vi.advanceTimersByTime(400 * 20);
    expect(events.length).toBe(count);
    expect(probe.engineInstalled).toBe(false);
    expect(refusal(() => h["local-ai.testModel"]({ model: LFM })).detail).toBe("notInstalled");
    expect(refusal(() => h["local-ai.testModel"]({ model: "nope" })).kind).toBe("notFound");
    expect(h["local-ai.cancelTest"]({})).toBeNull();
  });

  it("says the processor ran the test when models are kept there; a changed choice ends a running test", () => {
    const { h, events, probe } = setup();
    probe.engineInstalled = true;
    h["local-ai.setCpuOnly"]({ cpuOnly: true });
    h["local-ai.testModel"]({ model: E2B });
    vi.advanceTimersByTime(400 * 20);
    const last = tests(events).at(-1);
    if (last?.phase.kind !== "done") throw new Error(last?.phase.kind);
    expect(last.phase.result.gpu).toBe(false);
    h["local-ai.testModel"]({ model: E2B });
    vi.advanceTimersByTime(400 * 2);
    // the same choice again leaves the test alone
    h["local-ai.setCpuOnly"]({ cpuOnly: true });
    expect(tests(events).at(-1)?.phase.kind).toBe("loading");
    h["local-ai.setCpuOnly"]({ cpuOnly: false });
    expect(tests(events).at(-1)?.phase).toEqual({ kind: "cancelled" });
  });

  it("ends in the failure asked by the probe", () => {
    const { h, events, probe } = setup();
    probe.engine = NEWER;
    probe.engineInstalled = true;
    probe.failTest = { kind: "unavailable", subject: "engine", detail: "outOfMemory" };
    h["local-ai.testModel"]({ model: EMBED });
    vi.advanceTimersByTime(400 * 20);
    expect(kinds(events)).toEqual(["loading", "running", "failed"]);
    expect(tests(events).at(-1)?.phase).toEqual({
      kind: "failed",
      error: { kind: "unavailable", subject: "engine", detail: "outOfMemory" },
    });
  });
});

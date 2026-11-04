import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { InstallState } from "./generated/InstallState";
import type { LocalAiState } from "./generated/LocalAiState";
import { createMockLocalAi, MOCK_CATALOG } from "./mockLocalAi";
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

function setup() {
  const events: { name: AppPluginEventName; payload: unknown }[] = [];
  const mock = createMockLocalAi((name, payload) => events.push({ name, payload }));
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
    const { h, now } = setup();
    h["local-ai.get"]({ model: GERMAN });
    vi.advanceTimersByTime(400 * 4);
    h["local-ai.cancel"]({ model: GERMAN });
    const paused = now(GERMAN);
    expect(paused).toMatchObject({ kind: "paused" });
    vi.advanceTimersByTime(400 * 20);
    expect(now(GERMAN)).toEqual(paused);
    h["local-ai.deletePart"]({ model: GERMAN });
    expect(now(GERMAN)).toEqual({ kind: "notInstalled" });
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
    const { h, now } = setup();
    const error = refusal(() => h["local-ai.get"]({ model: LFM }));
    expect([error.kind, error.detail]).toEqual(["denied", "license"]);
    expect(now(LFM)).toEqual({ kind: "notInstalled" });
    const after = h["local-ai.acceptLicense"]({ model: LFM });
    expect(after.acceptedLicenses).toEqual(["https://www.liquid.ai/lfm-license"]);
    expect(after.models.find((m) => m.model === LFM)?.blocked).toBeNull();
    h["local-ai.get"]({ model: LFM });
    expect(now(LFM)).toEqual({ kind: "queued" });
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
    const { h, now } = setup();
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

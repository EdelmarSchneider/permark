// @vitest-environment jsdom
// The once-only offer across an in-place project switch (real-input int/RI-I3, 0.7.0): project A shares, the same
// view then shows project B (agent files, never asked). Whatever order A's and B's answers come in, B's offer shows
// once B's switch is known to be off, and nothing is recorded for B (S1).
import "../../core/testing/media";
import { flushSync, tick } from "svelte";
import { afterEach, beforeEach, expect, it } from "vitest";
import type { AgentStatus, Api, JsonValue, Result } from "../../api";
import { conditions, neutralContext } from "../../core/conditions/context.svelte";
import { PluginClient } from "../../core/plugins/client";
import { Box } from "../../core/testing/box.svelte";
import { AgentOffer, type SessionPort } from "./offer.svelte";
import { AgentStore } from "./store.svelte";

const A = "C:/tmp/pm07/docs";
const B = "C:/tmp/pm07/root";

const settle = async (): Promise<void> => {
  for (let i = 0; i < 5; i++) {
    await new Promise((resolve) => setTimeout(resolve, 0));
    flushSync();
  }
  await tick();
};

const statusOf = (enabled: boolean): AgentStatus => ({
  enabled,
  server: enabled ? { state: "listening" } : { state: "off" },
  clients: [],
  groups: {},
  denied: [],
  recent: [],
});

type Envelope = { root: string; data: unknown };

/** The backend as the view sees it: `agent.status` held until the test answers it, `agent:status` sent by hand. */
class Backend {
  readonly enabled = new Map<string, boolean>([
    [A, true],
    [B, false],
  ]);
  readonly answers = new Map<string, JsonValue | null>([
    [A, "setup"],
    [B, null],
  ]);
  readonly asked: { root: string; answer: (result: Result<AgentStatus>) => void }[] = [];
  readonly #listeners = new Set<(payload: Envelope) => void>();

  constructor(readonly root: Box<string | null>) {}

  readonly api = {
    pluginCall: (method: string): Promise<Result<unknown>> => {
      if (method !== "agent.status") return Promise.resolve({ ok: true, value: null });
      const root = this.root.value ?? "";
      return new Promise((resolve) => this.asked.push({ root, answer: resolve }));
    },
    onPlugin: (event: string, handler: (payload: Envelope) => void): Promise<() => void> => {
      if (event !== "agent:status") return Promise.resolve(() => {});
      this.#listeners.add(handler);
      return Promise.resolve(() => this.#listeners.delete(handler));
    },
  } as unknown as Api;

  readonly session: SessionPort = {
    projectSessionGet: () =>
      Promise.resolve({ ok: true, value: this.answers.get(this.root.value ?? "") ?? null }),
    projectSessionSet: (_key, value) => {
      this.answers.set(this.root.value ?? "", value);
      return Promise.resolve({ ok: true, value: null });
    },
  };

  /** Answers the oldest held `agent.status` with the status of the project it was asked for. */
  answerNext(): void {
    const next = this.asked.shift();
    next?.answer({ ok: true, value: statusOf(this.enabled.get(next.root) === true) });
  }

  emit(root: string, enabled: boolean): void {
    for (const listener of this.#listeners) listener({ root, data: statusOf(enabled) });
  }
}

let backend: Backend;
let offer: AgentOffer;
let agent: AgentStore;
let stops: (() => void)[] = [];

beforeEach(async () => {
  conditions.set({ ...neutralContext(), workspace: "project", agentFiles: ["CLAUDE.md"] });
  backend = new Backend(new Box<string | null>(A));
  const root = (): string | null => backend.root.value;
  agent = new AgentStore(
    new PluginClient(backend.api, root),
    root,
    () => ({ doc: null, mode: "view", section: null, selection: null, unsaved: false }),
    () => {},
  );
  offer = new AgentOffer(backend.session, root, agent);
  stops.push(agent.start(), offer.start());
  await settle();
  backend.answerNext();
  await settle();
  expect(agent.enabled).toBe(true);
  expect(offer.shown).toBe(false);
});

afterEach(() => {
  for (const stop of stops) stop();
  stops = [];
  conditions.set(neutralContext());
});

it("B's status comes late: no offer meanwhile, then the offer; nothing recorded", async () => {
  backend.root.value = B;
  await settle();
  expect(offer.shown).toBe(false);
  expect(agent.status).toBeNull();
  backend.answerNext();
  await settle();
  expect(offer.shown).toBe(true);
  expect(backend.answers.get(B)).toBeNull();
});

it("B's status is never sent as an event: the call alone shows the offer", async () => {
  backend.root.value = B;
  await settle();
  backend.answerNext();
  await settle();
  expect(agent.enabled).toBe(false);
  expect(offer.state).toEqual({ status: "unasked" });
  expect(offer.shown).toBe(true);
});

it("A's last status arrives after the switch (event and call answer): ignored", async () => {
  // A shown again (a call for A on its way), then B before that call is answered
  backend.root.value = null;
  await settle();
  backend.root.value = A;
  await settle();
  backend.root.value = B;
  await settle();
  expect(backend.asked.map((a) => a.root)).toEqual([A, B]);
  // A's sharing stopping, said after the view moved on
  backend.emit(A, true);
  await settle();
  expect(agent.status).toBeNull();
  backend.answerNext(); // A's call (on): dropped
  await settle();
  expect(agent.status).toBeNull();
  expect(backend.answers.get(B)).toBeNull();
  backend.answerNext(); // B's
  await settle();
  backend.emit(A, true);
  await settle();
  expect(agent.enabled).toBe(false);
  expect(offer.shown).toBe(true);
  expect(backend.answers.get(B)).toBeNull();
});

it("rapid A -> B -> A -> B: B's offer shows, A keeps its answer, B none", async () => {
  for (const next of [B, A, B]) {
    backend.root.value = next;
    flushSync();
  }
  await settle();
  // the held calls of the passed views answer in order; only the last one counts
  while (backend.asked.length > 0) backend.answerNext();
  await settle();
  expect(offer.shown).toBe(true);
  expect(backend.answers.get(A)).toBe("setup");
  expect(backend.answers.get(B)).toBeNull();
});

it("B's own switch turned on while unasked: recorded for B only (not from A's)", async () => {
  backend.root.value = B;
  await settle();
  backend.answerNext();
  await settle();
  backend.emit(B, true);
  await settle();
  expect(backend.answers.get(B)).toBe("setup");
  expect(offer.shown).toBe(false);
});

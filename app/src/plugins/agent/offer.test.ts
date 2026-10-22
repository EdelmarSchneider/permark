// @vitest-environment jsdom
// The once-only offer to share a project holding agent files: the decision, the answer kept per project, and what
// the two answers do (neither turns sharing on).
import "../../core/testing/media";
import { flushSync, tick } from "svelte";
import { afterEach, describe, expect, it } from "vitest";
import type { JsonValue, Result } from "../../api";
import { MOCK_FOLDER, createMockApi } from "../../api/mock";
import { conditions, neutralContext } from "../../core/conditions/context.svelte";
import { PluginClient } from "../../core/plugins/client";
import { Box } from "../../core/testing/box.svelte";
import { ui } from "../../core/shell/ui.svelte";
import { OFFER_KEY, offerStateOf, shouldOfferAgent, type OfferInput } from "./offer";
import { AGENT_PAGE, AgentOffer, type SessionPort } from "./offer.svelte";
import { AgentStore } from "./store.svelte";

const settle = async (): Promise<void> => {
  for (let i = 0; i < 5; i++) {
    await new Promise((resolve) => setTimeout(resolve, 0));
    flushSync();
  }
  await tick();
};

describe("whether the offer shows", () => {
  const yes: OfferInput = {
    workspace: "project",
    files: ["CLAUDE.md"],
    state: { status: "unasked" },
    enabled: false,
  };

  it("shows in a project with agent files, the switch known off, never answered", () => {
    expect(shouldOfferAgent(yes)).toBe(true);
    expect(shouldOfferAgent({ ...yes, files: [".claude/", "AGENTS.md"] })).toBe(true);
  });

  it("never shows anywhere else", () => {
    for (const workspace of ["welcome", "singleFile", "notes"] as const) {
      expect(shouldOfferAgent({ ...yes, workspace }), workspace).toBe(false);
    }
    expect(shouldOfferAgent({ ...yes, files: [] }), "no agent files").toBe(false);
    expect(shouldOfferAgent({ ...yes, enabled: true }), "already on").toBe(false);
    expect(shouldOfferAgent({ ...yes, enabled: null }), "switch not known yet").toBe(false);
    expect(shouldOfferAgent({ ...yes, state: { status: "loading" } })).toBe(false);
    expect(shouldOfferAgent({ ...yes, state: { status: "unreadable" } })).toBe(false);
    for (const answer of ["setup", "declined"] as const) {
      expect(shouldOfferAgent({ ...yes, state: { status: "answered", answer } }), answer).toBe(
        false,
      );
    }
  });

  it("reads the stored answer: absent = never asked; a value it does not know = answered", () => {
    expect(offerStateOf(null)).toEqual({ status: "unasked" });
    expect(offerStateOf(undefined)).toEqual({ status: "unasked" });
    expect(offerStateOf("setup")).toEqual({ status: "answered", answer: "setup" });
    expect(offerStateOf("declined")).toEqual({ status: "answered", answer: "declined" });
    for (const odd of ["later", 3, true, { v: 2 }, ""]) {
      expect(offerStateOf(odd).status, JSON.stringify(odd)).toBe("answered");
    }
  });
});

describe("the offer in a window", () => {
  let stops: (() => void)[] = [];

  afterEach(() => {
    for (const stop of stops) stop();
    stops = [];
    conditions.set(neutralContext());
    ui.closeSettingsPage();
  });

  async function setup(session?: (api: ReturnType<typeof createMockApi>) => SessionPort) {
    const api = createMockApi();
    await api.openPath(MOCK_FOLDER, "thisTab");
    conditions.set({ ...neutralContext(), workspace: "project", agentFiles: ["CLAUDE.md"] });
    const root = new Box<string | null>(MOCK_FOLDER);
    const agent = new AgentStore(
      new PluginClient(api, () => root.value),
      () => root.value,
      () => ({ doc: null, mode: "view", section: null, selection: null, unsaved: false }),
      () => {},
    );
    const offer = new AgentOffer(session ? session(api) : api, () => root.value, agent);
    stops.push(agent.start(), offer.start());
    await settle();
    return { api, agent, offer, root };
  }

  it("the mock starts answered: scenarios and fixtures look as before", async () => {
    const { offer } = await setup();
    expect(offer.state).toEqual({ status: "answered", answer: "declined" });
    expect(offer.shown).toBe(false);
  });

  it("shows once, until 'No thanks', which is kept for the project", async () => {
    const { api, offer, root } = await setup((api) => {
      void api.projectSessionSet(OFFER_KEY, null);
      return api;
    });
    expect(offer.state).toEqual({ status: "unasked" });
    expect(offer.shown).toBe(true);
    expect(offer.files).toEqual(["CLAUDE.md"]);
    await offer.decline();
    expect(offer.shown).toBe(false);
    expect((await api.projectSessionGet(OFFER_KEY)).ok).toBe(true);
    expect(await api.projectSessionGet(OFFER_KEY)).toEqual({ ok: true, value: "declined" });
    // the project loads again: still answered; new agent files never bring it back
    root.value = null;
    await settle();
    root.value = MOCK_FOLDER;
    conditions.set({ ...neutralContext(), workspace: "project", agentFiles: ["AGENTS.md"] });
    await settle();
    expect(offer.state).toEqual({ status: "answered", answer: "declined" });
    expect(offer.shown).toBe(false);
  });

  it("'Set up…' opens the section on its first control and turns nothing on", async () => {
    const { api, agent, offer } = await setup((api) => {
      void api.projectSessionSet(OFFER_KEY, null);
      return api;
    });
    await offer.setUp();
    expect(ui.settingsPage).toEqual({ status: "open", section: AGENT_PAGE });
    expect(agent.enabled).toBe(false);
    expect(api.probe.plugins.agent.status().enabled).toBe(false);
    expect(await api.projectSessionGet(OFFER_KEY)).toEqual({ ok: true, value: "setup" });
    expect(offer.shown).toBe(false);
    // the request for focus is spent once
    expect(ui.takeContentFocus(AGENT_PAGE)).toBe(true);
    expect(ui.takeContentFocus(AGENT_PAGE)).toBe(false);
  });

  it("the switch turned on by any path while unasked ends the offer as 'setup'", async () => {
    const { api, agent, offer } = await setup((api) => {
      void api.projectSessionSet(OFFER_KEY, null);
      return api;
    });
    expect(offer.shown).toBe(true);
    await agent.setEnabled(true);
    await settle();
    expect(offer.state).toEqual({ status: "answered", answer: "setup" });
    expect(await api.projectSessionGet(OFFER_KEY)).toEqual({ ok: true, value: "setup" });
    await agent.setEnabled(false);
    await settle();
    expect(offer.shown).toBe(false);
  });

  it("a key that cannot be read offers nothing", async () => {
    const { offer } = await setup((api) => ({
      projectSessionGet: () =>
        Promise.resolve<Result<JsonValue | null>>({
          ok: false,
          error: { kind: "io", subject: null, detail: null },
        } as unknown as Result<JsonValue | null>),
      projectSessionSet: (key, value) => api.projectSessionSet(key, value),
    }));
    expect(offer.state).toEqual({ status: "unreadable" });
    expect(offer.shown).toBe(false);
  });

  it("an answer read for a project the window has left is not taken", async () => {
    const pending: ((value: Result<JsonValue | null>) => void)[] = [];
    const { offer, root } = await setup((api) => ({
      projectSessionGet: () =>
        new Promise<Result<JsonValue | null>>((resolve) => pending.push(resolve)),
      projectSessionSet: (key, value) => api.projectSessionSet(key, value),
    }));
    expect(offer.state).toEqual({ status: "loading" });
    root.value = "C:/Users/demo/other";
    await settle();
    // the first project's answer arrives late: ignored; the second's is taken
    pending[0]?.({ ok: true, value: null });
    await settle();
    expect(offer.state).toEqual({ status: "loading" });
    pending[1]?.({ ok: true, value: "declined" });
    await settle();
    expect(offer.state).toEqual({ status: "answered", answer: "declined" });
  });
});

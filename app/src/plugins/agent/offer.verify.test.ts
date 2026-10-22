// @vitest-environment jsdom
// Verifier (integration 0.7): once-only offer x agent switch of the project shown before. A view that moves from a
// project with sharing on to another one (never asked) must not record "setup" for the second one from the first
// one's switch, while the second one's status is still on its way.
import "../../core/testing/media";
import { flushSync, tick } from "svelte";
import { afterEach, expect, it } from "vitest";
import type { Api, JsonValue, Result } from "../../api";
import { MOCK_FOLDER, createMockApi } from "../../api/mock";
import { conditions, neutralContext } from "../../core/conditions/context.svelte";
import { PluginClient } from "../../core/plugins/client";
import { Box } from "../../core/testing/box.svelte";
import { AgentOffer, type SessionPort } from "./offer.svelte";
import { AgentStore } from "./store.svelte";

const settle = async (): Promise<void> => {
  for (let i = 0; i < 5; i++) {
    await new Promise((resolve) => setTimeout(resolve, 0));
    flushSync();
  }
  await tick();
};

let stops: (() => void)[] = [];
afterEach(() => {
  for (const stop of stops) stop();
  stops = [];
  conditions.set(neutralContext());
});

it("moving from a shared project to an unasked one does not answer the second one's offer", async () => {
  const api = createMockApi();
  await api.openPath(MOCK_FOLDER, "thisTab");
  conditions.set({ ...neutralContext(), workspace: "project", agentFiles: ["CLAUDE.md"] });
  // agent.status of the second project is held until released
  let slow = false;
  let release: () => void = () => {};
  const held = new Promise<void>((resolve) => (release = resolve));
  const slowApi = new Proxy(api, {
    get(target, prop, receiver) {
      if (prop === "pluginCall") {
        return (method: string, args: unknown) => {
          const call = (): unknown =>
            (target.pluginCall as (m: string, a: unknown) => unknown)(method, args);
          if (!(slow && method === "agent.status")) return call();
          // the second project's switch is off
          return held.then(call).then((r) => {
            const result = r as { ok: boolean; value?: Record<string, unknown> };
            return result.ok ? { ...result, value: { ...result.value, enabled: false } } : result;
          });
        };
      }
      return Reflect.get(target, prop, receiver) as unknown;
    },
  }) as Api;
  // per project answers: the first answered "setup", the second never asked
  const answers = new Map<string, JsonValue | null>([
    [MOCK_FOLDER, "setup"],
    ["C:/Users/demo/other", null],
  ]);
  const root = new Box<string | null>(MOCK_FOLDER);
  const session: SessionPort = {
    projectSessionGet: () =>
      Promise.resolve<Result<JsonValue | null>>({
        ok: true,
        value: answers.get(root.value ?? "") ?? null,
      }),
    projectSessionSet: (_key, value) => {
      answers.set(root.value ?? "", value);
      return Promise.resolve({ ok: true, value: null });
    },
  };
  const agent = new AgentStore(
    new PluginClient(slowApi, () => root.value),
    () => root.value,
    () => ({ doc: null, mode: "view", section: null, selection: null, unsaved: false }),
    () => {},
  );
  const offer = new AgentOffer(session, () => root.value, agent);
  stops.push(agent.start(), offer.start());
  await settle();
  await agent.setEnabled(true);
  await settle();
  expect(agent.enabled).toBe(true);
  slow = true;
  root.value = "C:/Users/demo/other";
  await settle();
  release();
  await settle();
  expect(answers.get("C:/Users/demo/other")).toBeNull();
  expect(offer.state).toEqual({ status: "unasked" });
});

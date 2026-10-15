import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

// The backend is replaced: these tests are about what the window tells it, when, and what the switch does first.
const fake = vi.hoisted(() => ({
  agentStatus: vi.fn(),
  setAgentConnection: vi.fn(),
  publishAgentContext: vi.fn(),
  saveSession: vi.fn(),
}));
vi.mock("../api", () => ({ api: { ...fake, on: async () => () => {} } }));

import { bridge } from "../bridge";
import type { AgentStatus, DocumentView } from "../types";
import { agent, PUBLISH_DELAY_MS } from "./agent.svelte";
import { docs } from "./docs.svelte";
import { session } from "./session.svelte";
import { toast } from "./toast.svelte";

const OFF: AgentStatus = {
  enabled: false,
  listening: false,
  connections: 0,
  recent: [],
  error: null,
  program: "/usr/bin/trailmark",
};
const ON: AgentStatus = { ...OFF, enabled: true, listening: true };

function openDocument(path: string | null): void {
  docs.current = path === null ? null : ({ path, kind: "markdown" } as DocumentView);
}

beforeEach(() => {
  vi.useFakeTimers();
  for (const fn of Object.values(fake)) fn.mockReset();
  fake.agentStatus.mockResolvedValue(OFF);
  fake.setAgentConnection.mockImplementation(async (on: boolean) => (on ? ON : OFF));
  fake.publishAgentContext.mockResolvedValue(undefined);
  fake.saveSession.mockResolvedValue(undefined);
  session.load(null);
  agent.reset();
  openDocument("docs/plan.md");
  bridge.readingSnapshot = () => ({
    headings: [
      { level: 1, text: "Plan", line: 1 },
      { level: 2, text: "Rollout", line: 5 },
    ],
    lastLine: 9,
    selection: { text: "ship it", startLine: 6, endLine: 6 },
  });
  toast.clear();
});

afterEach(() => {
  bridge.readingSnapshot = undefined;
  openDocument(null);
  vi.useRealTimers();
});

describe("the switch", () => {
  it("saves the setting first, so the programs that read the session find it, then starts the connection", async () => {
    const order: string[] = [];
    fake.saveSession.mockImplementation(async (s: { agentConnection: boolean }) => {
      order.push(`saved ${s.agentConnection}`);
    });
    fake.setAgentConnection.mockImplementation(async (on: boolean) => {
      order.push(`backend ${on}`);
      return ON;
    });
    await agent.setEnabled(true);
    expect(order).toEqual(["saved true", "backend true"]);
    expect(session.agentConnection).toBe(true);
    expect(agent.enabled).toBe(true);
  });

  it("turns it off the same way, and forgets who was connected", async () => {
    await agent.setEnabled(true);
    await agent.setEnabled(false);
    expect(session.agentConnection).toBe(false);
    expect(agent.enabled).toBe(false);
    expect(fake.setAgentConnection).toHaveBeenLastCalledWith(false);
  });

  it("says so when the backend refuses, and leaves the switch as the backend has it", async () => {
    fake.setAgentConnection.mockRejectedValue({ kind: "internal", message: "no socket" });
    await agent.setEnabled(true);
    expect(toast.current).toMatchObject({ type: "message", tone: "warn" });
    expect(agent.busy).toBe(false);
    expect(agent.enabled).toBe(false);
  });

  it("toggles", async () => {
    await agent.toggle();
    expect(agent.enabled).toBe(true);
    await agent.toggle();
    expect(agent.enabled).toBe(false);
  });
});

describe("the message", () => {
  it("is sent a moment after the connection is turned on, with the section on screen and the selection", async () => {
    await agent.setEnabled(true);
    expect(fake.publishAgentContext).not.toHaveBeenCalled();
    await vi.advanceTimersByTimeAsync(1);
    expect(fake.publishAgentContext).toHaveBeenCalledTimes(1);
    expect(fake.publishAgentContext).toHaveBeenCalledWith({
      document: "docs/plan.md",
      mode: "reading",
      section: null, // no section is being read: the outline has not said which
      selection: { text: "ship it", headingPath: ["Plan", "Rollout"], startLine: 6, endLine: 6 },
      unsaved: false,
    });
  });

  it("waits for the changes to stop, and sends one message for a burst", async () => {
    await agent.setEnabled(true);
    await vi.advanceTimersByTimeAsync(1);
    fake.publishAgentContext.mockClear();
    bridge.readingSnapshot = () => ({
      headings: [],
      lastLine: 1,
      selection: { text: "first" },
    });
    agent.schedule();
    await vi.advanceTimersByTimeAsync(PUBLISH_DELAY_MS - 10);
    bridge.readingSnapshot = () => ({
      headings: [],
      lastLine: 1,
      selection: { text: "second" },
    });
    agent.schedule();
    await vi.advanceTimersByTimeAsync(PUBLISH_DELAY_MS - 10);
    expect(fake.publishAgentContext).not.toHaveBeenCalled();
    await vi.advanceTimersByTimeAsync(20);
    expect(fake.publishAgentContext).toHaveBeenCalledTimes(1);
    expect(fake.publishAgentContext.mock.calls[0][0].selection.text).toBe("second");
  });

  it("is not sent again when nothing changed", async () => {
    await agent.setEnabled(true);
    await vi.advanceTimersByTimeAsync(1);
    agent.schedule();
    await vi.advanceTimersByTimeAsync(PUBLISH_DELAY_MS + 1);
    agent.schedule();
    await vi.advanceTimersByTimeAsync(PUBLISH_DELAY_MS + 1);
    expect(fake.publishAgentContext).toHaveBeenCalledTimes(1);
  });

  it("says nothing is open when no document is", async () => {
    openDocument(null);
    await agent.setEnabled(true);
    await vi.advanceTimersByTimeAsync(1);
    expect(fake.publishAgentContext).toHaveBeenCalledWith({
      document: null,
      mode: null,
      section: null,
      selection: null,
      unsaved: false,
    });
  });

  it("is never sent while the connection is off", async () => {
    agent.schedule();
    await vi.advanceTimersByTimeAsync(PUBLISH_DELAY_MS * 3);
    expect(fake.publishAgentContext).not.toHaveBeenCalled();
    await agent.setEnabled(true);
    await agent.setEnabled(false);
    fake.publishAgentContext.mockClear();
    agent.schedule();
    await vi.advanceTimersByTimeAsync(PUBLISH_DELAY_MS * 3);
    expect(fake.publishAgentContext).not.toHaveBeenCalled();
  });

  it("is sent again after a failure, with the next change", async () => {
    await agent.setEnabled(true);
    fake.publishAgentContext.mockRejectedValueOnce(new Error("busy"));
    await vi.advanceTimersByTimeAsync(1);
    expect(fake.publishAgentContext).toHaveBeenCalledTimes(1);
    agent.schedule();
    await vi.advanceTimersByTimeAsync(PUBLISH_DELAY_MS + 1);
    expect(fake.publishAgentContext).toHaveBeenCalledTimes(2);
  });

  it("is dropped when the project changes while it is being made", async () => {
    await agent.setEnabled(true);
    agent.schedule(0);
    agent.reset();
    await vi.advanceTimersByTimeAsync(PUBLISH_DELAY_MS * 2);
    expect(fake.publishAgentContext).not.toHaveBeenCalled();
  });
});

describe("the status", () => {
  it("shows an agent as connected only while the connection listens", () => {
    agent.onStatus({ ...ON, connections: 1 });
    expect(agent.connected).toBe(true);
    agent.onStatus({ ...ON, connections: 0 });
    expect(agent.connected).toBe(false);
    agent.onStatus({ ...OFF, connections: 1 });
    expect(agent.connected).toBe(false);
  });

  it("pulses for a request that is new, not for the ones already there", async () => {
    const request = (atMs: number) => ({ tool: "get_context", subject: null, atMs });
    agent.onStatus({ ...ON, connections: 1, recent: [request(1000)] });
    expect(agent.pulsing).toBe(false);
    agent.onStatus({ ...ON, connections: 1, recent: [request(2000), request(1000)] });
    expect(agent.pulsing).toBe(true);
    await vi.advanceTimersByTimeAsync(1000);
    expect(agent.pulsing).toBe(false);
    // the same status again is no new request
    agent.onStatus({ ...ON, connections: 1, recent: [request(2000), request(1000)] });
    expect(agent.pulsing).toBe(false);
  });

  it("starts from what the backend says when a project opens", async () => {
    fake.agentStatus.mockResolvedValue({ ...ON, connections: 1 });
    await agent.onProjectOpened();
    expect(agent.enabled).toBe(true);
    expect(agent.connected).toBe(true);
    expect(agent.status.program).toBe("/usr/bin/trailmark");
  });
});

// @vitest-environment jsdom
// The assistant's store against the mock backend: a message and its streamed reply (text in pieces, one line per
// tool, a question answered), the changes read at the turn's end and taken hunk by hunk, Stop, a new conversation,
// refusals in words, and the passage attached from the open document (frozen while the panel has the keyboard).
import "../../core/testing/media";
import { flushSync, tick } from "svelte";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { ApiError } from "../../api";
import { MOCK_FOLDER, createMockApi } from "../../api/mock";
import { MockPluginError } from "../../api/mockOrganizing";
import { documents } from "../../core/doc/document.svelte";
import { editing } from "../../core/editing/session.svelte";
import { apiErrorText } from "../../core/errors";
import { t } from "../../core/i18n";
import { PluginClient } from "../../core/plugins/client";
import {
  AcpStore,
  agentLabel,
  errorText,
  linesOf,
  PANEL_MARK,
  passageLabel,
  type Entry,
} from "./store.svelte";

const settle = async (): Promise<void> => {
  for (let i = 0; i < 5; i++) {
    await new Promise((resolve) => setTimeout(resolve, 0));
    flushSync();
  }
  await tick();
};

async function until(done: () => boolean): Promise<void> {
  for (let i = 0; i < 400 && !done(); i++) await new Promise((resolve) => setTimeout(resolve, 2));
  flushSync();
  expect(done()).toBe(true);
}

const stops: (() => void)[] = [];

async function setup() {
  const api = createMockApi();
  await api.openPath(MOCK_FOLDER, "thisTab");
  api.probe.plugins.acp.stepMs = 1;
  const store = new AcpStore(new PluginClient(api, () => MOCK_FOLDER), () => MOCK_FOLDER);
  stops.push(store.start(), () => api.probe.plugins.acp.stopAll());
  await settle();
  return { api, store, acp: api.probe.plugins.acp };
}

const backend = (kind: MockPluginError["kind"], detail: string | null): ApiError => ({
  kind: "backend",
  command: "plugin_call",
  error: { kind, subject: "docs/guide.md", detail },
});

const kinds = (entries: readonly Entry[]): string[] => entries.map((e) => e.kind);

afterEach(() => {
  for (const stop of stops.splice(0)) stop();
  vi.restoreAllMocks();
  document.body.replaceChildren();
  editing.mode = "view";
  editing.selection = "";
});

describe("a message and its reply", () => {
  it("streams into one conversation: text in pieces, a line per tool, a question, then the changes", async () => {
    const { store, acp } = await setup();
    expect(store.status).toEqual(expect.objectContaining({ state: "idle", error: null }));
    store.passage = { path: "docs/guide.md", fromLine: 7, toLine: 7, text: "Open a folder" };
    store.draft = "Make this clearer";
    await store.send();
    // the message is in at once; the box and the passage are free for the next one
    expect(store.transcript[0]).toEqual(
      expect.objectContaining({ kind: "you", text: "Make this clearer" }),
    );
    expect(store.draft).toBe("");
    expect(store.passage).toBeNull();
    expect(acp.prompts[0]?.selection).toEqual(expect.objectContaining({ fromLine: 7, toLine: 7 }));
    expect(store.busy).toBe(true);

    await until(() => store.transcript.some((e) => e.kind === "question"));
    expect(kinds(store.transcript)).toEqual([
      "you",
      "thinking",
      "tool",
      "agent",
      "tool",
      "question",
    ]);
    const agent = store.transcript.find((e) => e.kind === "agent");
    expect(agent?.kind === "agent" && agent.text).toBe(
      "I read lines 7-7 of docs/guide.md. Two places can be clearer: the first steps, and the list of shortcuts.\n",
    );
    // a tool's news updates its line, never adds one
    const tools = store.transcript.filter((e) => e.kind === "tool");
    expect(tools.map((e) => e.kind === "tool" && e.status)).toEqual(["completed", "completed"]);

    const question = store.transcript.find((e) => e.kind === "question");
    if (question?.kind !== "question") throw new Error("no question");
    const allow = question.options.find((o) => o.kind === "allow_once");
    if (allow === undefined) throw new Error("no allow option");
    await store.answer(question.requestId, allow);
    expect(acp.answers).toEqual([{ requestId: question.requestId, optionId: allow.id }]);

    await until(() => store.status?.state === "ready" && store.changes.length === 1);
    const answered = store.transcript.find((e) => e.kind === "question");
    // the news that the agent no longer waits leaves the given answer as it was
    expect(answered?.kind === "question" && answered.answer).toEqual({
      status: "answered",
      option: "Allow",
    });
    expect(store.changes[0]?.hunks).toHaveLength(2);
    expect(store.busy).toBe(false);
  });

  it("a hunk accepted leaves the other, moved by the lines it added; a file rejected leaves nothing", async () => {
    const { store, acp } = await setup();
    acp.changes = [
      {
        path: "docs/guide.md",
        kind: "modified",
        hunks: [
          { id: 0, oldStart: 7, oldLines: ["a"], newLines: ["b", "c"] },
          { id: 1, oldStart: 17, oldLines: ["d"], newLines: ["e"] },
        ],
      },
    ];
    await store.refreshChanges();
    await store.decide("accept", "docs/guide.md", [0]);
    expect(store.changes[0]?.hunks).toEqual([
      { id: 0, oldStart: 18, oldLines: ["d"], newLines: ["e"] },
    ]);
    await store.decide("reject", "docs/guide.md", null);
    expect(store.changes).toEqual([]);
    expect(store.refusal).toBeNull();
  });

  it("a refused accept says why beside the list and keeps it", async () => {
    const { store, acp } = await setup();
    acp.changes = [
      {
        path: "docs/guide.md",
        kind: "modified",
        hunks: [{ id: 0, oldStart: 7, oldLines: ["a"], newLines: ["b"] }],
      },
    ];
    await store.refreshChanges();
    acp.fail = new MockPluginError("conflict", "docs/guide.md", "unsaved");
    await store.decide("accept", "docs/guide.md", [0]);
    expect(store.refusal).toEqual({ path: "docs/guide.md", text: t("acp.error.unsaved") });
    expect(store.changes).toHaveLength(1);
    // an outdated hunk: said, and the list read again
    await store.decide("accept", "docs/guide.md", [5]);
    expect(store.refusal?.text).toBe(t("acp.error.outdated"));
    expect(acp.calls.filter((c) => c === "changes").length).toBeGreaterThanOrEqual(3);
  });

  it("Stop ends the turn: the open question closes and the conversation says so", async () => {
    const { store } = await setup();
    store.draft = "Go";
    await store.send();
    await until(() => store.transcript.some((e) => e.kind === "question"));
    await store.cancel();
    await settle();
    const question = store.transcript.find((e) => e.kind === "question");
    expect(question?.kind === "question" && question.answer).toEqual({ status: "closed" });
    expect(store.transcript.at(-1)).toEqual(
      expect.objectContaining({ kind: "notice", text: t("acp.stopped") }),
    );
    expect(store.status?.state).toBe("ready");
  });

  it("a new conversation clears the lines and keeps the changes", async () => {
    const { store, acp } = await setup();
    store.draft = "Go";
    await store.send();
    await until(() => store.transcript.some((e) => e.kind === "question"));
    acp.changes = [{ path: "a.md", kind: "added", hunks: [] }];
    await store.refreshChanges();
    await store.newConversation();
    await settle();
    expect(store.transcript).toEqual([]);
    expect(store.changes).toHaveLength(1);
    expect(store.status?.state).toBe("idle");
  });

  it("a message refused comes back to the box with its passage, and the refusal is said", async () => {
    const { store } = await setup();
    store.draft = "First";
    await store.send();
    // a second window's message while this one works: the backend refuses it
    store.take({ type: "status", state: "ready", error: null });
    store.draft = "Second";
    store.passage = { path: "a.md", fromLine: 1, toLine: 2, text: "x" };
    await store.send();
    expect(store.draft).toBe("Second");
    expect(store.passage).toEqual(expect.objectContaining({ path: "a.md" }));
    expect(store.transcript.at(-1)).toEqual(
      expect.objectContaining({
        kind: "notice",
        text: t("acp.notSent", { reason: t("acp.error.busy") }),
      }),
    );
  });

  it("the agent's own error text shows as a line; thinking shows once per stretch", () => {
    const store = new AcpStore();
    store.take({ type: "thinking" });
    store.take({ type: "thinking" });
    store.take({ type: "status", state: "error", error: "The agent stopped." });
    expect(kinds(store.transcript)).toEqual(["thinking", "notice"]);
    expect(store.status).toEqual({ state: "error", error: "The agent stopped.", agent: "" });
  });
});

describe("refusals in words", () => {
  it("each refusal of the plugin has its own words; others are worded as the app words them", () => {
    expect(errorText(backend("conflict", "busy"))).toBe(t("acp.error.busy"));
    expect(errorText(backend("conflict", "changed"))).toBe(t("acp.error.changed"));
    expect(errorText(backend("invalidData", "wholeFileOnly"))).toBe(t("acp.error.wholeFileOnly"));
    expect(errorText(backend("notFound", "noChange"))).toBe(t("acp.error.outdated"));
    expect(errorText(backend("notFound", "noQuestion"))).toBe(t("acp.error.noQuestion"));
    expect(errorText(backend("invalidPath", null))).toBe(t("acp.error.path"));
    expect(errorText(backend("denied", null))).toBe(apiErrorText(backend("denied", null)));
  });
});

describe("the attached passage", () => {
  function showing(mode: "view" | "edit" | "source"): void {
    vi.spyOn(documents, "path", "get").mockReturnValue("docs/a.md");
    vi.spyOn(editing, "path", "get").mockReturnValue("docs/a.md");
    editing.mode = mode;
  }

  it("in Source | Edit is the selection with the lines it covers in the edit text", () => {
    showing("source");
    vi.spyOn(editing, "current").mockReturnValue("alpha\nbeta\ngamma\ndelta");
    vi.spyOn(editing, "scope").mockReturnValue({ from: 6, to: 17 });
    editing.selection = "beta\ngamma\n";
    const store = new AcpStore();
    store.capture();
    expect(store.passage).toEqual({
      path: "docs/a.md",
      fromLine: 2,
      toLine: 3,
      text: "beta\ngamma\n",
    });
    expect(passageLabel(store.passage ?? { path: "", fromLine: 0, toLine: 0, text: "" })).toBe(
      t("acp.passage", { path: "docs/a.md", from: 2, to: 3 }),
    );
  });

  it("in View is the page selection with the lines of its blocks", () => {
    showing("view");
    vi.spyOn(documents, "referenceAt").mockReturnValue({
      path: "docs/a.md",
      section: null,
      line: 4,
    });
    const block = document.createElement("p");
    block.setAttribute("data-b", "b1");
    block.textContent = "A passage to send";
    document.body.append(block);
    const range = document.createRange();
    range.selectNodeContents(block);
    document.getSelection()?.removeAllRanges();
    document.getSelection()?.addRange(range);
    const store = new AcpStore();
    store.capture();
    expect(store.passage).toEqual({
      path: "docs/a.md",
      fromLine: 4,
      toLine: 4,
      text: "A passage to send",
    });
    expect(passageLabel({ path: "docs/a.md", fromLine: 4, toLine: 4, text: "" })).toBe(
      t("acp.passageLine", { path: "docs/a.md", line: 4 }),
    );
  });

  it("stays as it is while the panel has the keyboard; nothing selected means none", () => {
    showing("source");
    const current = vi.spyOn(editing, "current").mockReturnValue("alpha\nbeta");
    const scope = vi.spyOn(editing, "scope").mockReturnValue({ from: 0, to: 5 });
    editing.selection = "alpha";
    const store = new AcpStore();
    store.capture();
    expect(store.passage?.fromLine).toBe(1);
    const panel = document.createElement("section");
    panel.setAttribute(PANEL_MARK, "");
    const box = document.createElement("textarea");
    panel.append(box);
    document.body.append(panel);
    box.focus();
    scope.mockReturnValue(null);
    store.capture();
    expect(store.passage?.text).toBe("alpha");
    // back in the document: the selection there counts again
    box.blur();
    document.getSelection()?.removeAllRanges();
    store.capture();
    expect(store.passage).toBeNull();
    current.mockRestore();
  });
});

describe("words of the panel", () => {
  it("counts lines across line breaks, a range ending after a break ends on that line", () => {
    const text = "one\r\ntwo\nthree\n";
    expect(linesOf(text, { from: 0, to: 3 })).toEqual([1, 1]);
    expect(linesOf(text, { from: 5, to: 9 })).toEqual([2, 2]);
    expect(linesOf(text, { from: 5, to: 14 })).toEqual([2, 3]);
    expect(linesOf(text, { from: 15, to: 15 })).toEqual([4, 4]);
  });

  it("names the agent by its program or package", () => {
    expect(agentLabel("npx -y @agentclientprotocol/claude-agent-acp@0.108.0")).toBe(
      "claude-agent-acp",
    );
    expect(agentLabel("gemini --experimental-acp")).toBe("gemini");
    expect(agentLabel("C:\\tools\\codex-acp.exe")).toBe("codex-acp");
    expect(agentLabel("node C:/agents/my-agent/dist/index.js")).toBe("index");
  });
});

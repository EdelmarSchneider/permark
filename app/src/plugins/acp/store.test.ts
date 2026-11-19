// @vitest-environment jsdom
// The assistant's store against the mock backend: a message and its streamed reply (text in pieces, one line per
// tool, a question answered), the changes read at the turn's end and taken hunk by hunk, Stop, a new conversation,
// refusals in words, and the passage attached from the open document (frozen while the panel has the keyboard); the
// choice of the agent (none at first: the picker; a choice read back by name; refusals) and a sign-in the agent asks
// for (Try again alone stays signed out, the agent's own method signs in and the reply goes on); another project
// shown meanwhile (its news and late answers never reach it), the announcements, Undo after a decision, the caps; the
// agent's settings (its model, effort, a fast mode): shown while it runs, changed through the plugin, refusals in words.
import "../../core/testing/media";
import { flushSync, tick } from "svelte";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { AcpErrorCode, AgentEntry, ApiError, ChangeList, Result } from "../../api";
import { MOCK_FOLDER, createMockApi } from "../../api/mock";
import {
  MOCK_AGENTS,
  MOCK_LOGIN_METHODS,
  MOCK_NOT_SIGNED_IN,
  type MockAcpChoice,
} from "../../api/mockAcp";
import { MockPluginError } from "../../api/mockOrganizing";
import { documents } from "../../core/doc/document.svelte";
import { editing } from "../../core/editing/session.svelte";
import { apiErrorText } from "../../core/errors";
import { t } from "../../core/i18n";
import { PluginClient } from "../../core/plugins/client";
import { defaultArrangement, intoGroup, OUTLINE } from "../../core/shell/arrangement";
import { Box } from "../../core/testing/box.svelte";
import {
  AcpStore,
  canStart,
  configLabel,
  entryAbout,
  entryName,
  errorText,
  failureText,
  howKey,
  linesOf,
  PANEL_MARK,
  passageLabel,
  pickerList,
  previewText,
  firstPlace,
  MAX_ENTRIES,
  PANEL,
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

/** The mock project with the store following it; `choice`: the agent chosen before (`null`: none, a first run).
 *  `root`: the project the window shows (a test changes it to show another). */
async function setup(
  choice: MockAcpChoice | null = { agent: "claude-acp" },
  root = new Box<string | null>(MOCK_FOLDER),
) {
  const api = createMockApi();
  await api.openPath(MOCK_FOLDER, "thisTab");
  api.probe.plugins.acp.stepMs = 1;
  api.probe.plugins.acp.choice = choice;
  const client = new PluginClient(api, () => root.value);
  const say = vi.fn<(text: string) => void>();
  const notifyKeyed = vi.fn();
  const store = new AcpStore(client, () => root.value, { say, notices: { notifyKeyed } });
  stops.push(store.start(), () => api.probe.plugins.acp.stopAll());
  await settle();
  return { api, store, acp: api.probe.plugins.acp, client, root, say, notifyKeyed };
}

/** The next call of `method` waits until the test answers it (a late answer); later calls answer as always. */
function hold(client: PluginClient, method: string) {
  let answer: ((result: Result<unknown>) => void) | null = null;
  let held = false;
  const call = client.call.bind(client);
  vi.spyOn(client, "call").mockImplementation(((m: string, args: never) => {
    if (m !== method || held) return call(m as never, args);
    held = true;
    return new Promise((resolve) => {
      answer = resolve;
    });
  }) as never);
  return { answer: (result: Result<unknown>) => answer?.(result) };
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
    // "Thinking…" is one indicator beside the lines, gone once the agent says | does something
    expect(kinds(store.transcript)).toEqual(["you", "tool", "agent", "tool", "question"]);
    expect(store.thinking).toBe(false);
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
      option: t("acp.option.allowOnce"),
    });
    expect(store.changes[0]?.hunks).toHaveLength(2);
    expect(store.busy).toBe(false);
  });

  it("a finished message renders through the plugin as document blocks; one past the cap stays plain", async () => {
    const { store, acp } = await setup();
    const blocks = await store.render(
      "## Done\n\nSee `docs/guide.md:3` and [the guide](docs/guide.md).",
    );
    expect(acp.calls).toContain("render");
    expect(blocks?.map((b) => b.kind)).toEqual(["h2", "p"]);
    expect(blocks?.[1]?.html).toContain('<a href="docs/guide.md">the guide</a>');
    expect(blocks?.[1]?.html).toContain("<code>docs/guide.md:3</code>");
    expect(await store.render("x".repeat(256 * 1024 + 1))).toBeNull();
  });

  it("a hunk accepted leaves the other, moved by the lines it added; a file rejected leaves nothing", async () => {
    const { store, acp } = await setup();
    acp.changes = [
      {
        path: "docs/guide.md",
        kind: "modified",
        hunks: [
          {
            key: "k7",
            oldStart: 7,
            oldLines: ["a"],
            newLines: ["b", "c"],
            truncated: false,
            contextBefore: null,
            contextAfter: null,
          },
          {
            key: "k17",
            oldStart: 17,
            oldLines: ["d"],
            newLines: ["e"],
            truncated: false,
            contextBefore: null,
            contextAfter: null,
          },
        ],
      },
    ];
    await store.refreshChanges();
    await store.decide("accept", "docs/guide.md", ["k7"]);
    expect(store.changes[0]?.hunks).toEqual([
      {
        key: "k17",
        oldStart: 18,
        oldLines: ["d"],
        newLines: ["e"],
        truncated: false,
        contextBefore: null,
        contextAfter: null,
      },
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
        hunks: [
          {
            key: "k7",
            oldStart: 7,
            oldLines: ["a"],
            newLines: ["b"],
            truncated: false,
            contextBefore: null,
            contextAfter: null,
          },
        ],
      },
    ];
    await store.refreshChanges();
    acp.fail = new MockPluginError("conflict", "docs/guide.md", "unsaved");
    await store.decide("accept", "docs/guide.md", ["k7"]);
    expect(store.refusal).toEqual({ path: "docs/guide.md", text: t("acp.error.unsaved") });
    expect(store.changes).toHaveLength(1);
    // a key of an older list: said, and the list read again
    await store.decide("accept", "docs/guide.md", ["old"]);
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

  it("a passage from a file agents may not see goes at the refusal and is never attached again; the message comes back", async () => {
    const { store, acp: probe } = await setup();
    probe.hidden.add("docs/secret.md");
    store.draft = "Summarize";
    store.passage = { path: "docs/secret.md", fromLine: 1, toLine: 1, text: "TOP SECRET" };
    await store.send();
    expect(store.passage).toBeNull();
    expect(store.draft).toBe("Summarize");
    expect(store.transcript.at(-1)).toEqual(
      expect.objectContaining({
        kind: "notice",
        text: t("acp.notSent", { reason: t("acp.error.hiddenFromAgents") }),
      }),
    );
    // a new selection in that file stays unattached; another file's attaches as always
    vi.spyOn(editing, "path", "get").mockReturnValue("docs/secret.md");
    const shown = vi.spyOn(documents, "path", "get").mockReturnValue("docs/secret.md");
    editing.mode = "source";
    vi.spyOn(editing, "current").mockReturnValue("TOP SECRET\nmore");
    vi.spyOn(editing, "scope").mockReturnValue({ from: 0, to: 10 });
    editing.selection = "TOP SECRET";
    store.capture();
    expect(store.passage).toBeNull();
    shown.mockReturnValue("docs/a.md");
    vi.spyOn(editing, "path", "get").mockReturnValue("docs/a.md");
    store.capture();
    expect(store.passage?.path).toBe("docs/a.md");
  });

  it("a failure shows as a line in words; thinking is one indicator, replaced by the text", () => {
    const store = new AcpStore();
    store.take({ type: "thinking" });
    store.take({ type: "thinking" });
    expect(store.thinking).toBe(true);
    expect(store.transcript).toEqual([]);
    store.take({ type: "text", text: "Hi" });
    expect(store.thinking).toBe(false);
    store.take({ type: "thinking" });
    store.take({ type: "status", state: "error", error: { code: "exited" } });
    expect(store.thinking).toBe(false);
    expect(kinds(store.transcript)).toEqual(["agent", "notice"]);
    expect(store.transcript.at(-1)).toEqual(
      expect.objectContaining({ kind: "notice", text: t("acp.fail.exited"), details: null }),
    );
    expect(store.status).toEqual({
      state: "error",
      error: { code: "exited" },
      agent: "",
      agentName: null,
      chosen: false,
      methods: [],
      skippedFiles: 0,
      last: null,
      options: [],
    });
  });
});

describe("continuing the last conversation", () => {
  const ready = (store: AcpStore) => (): boolean => store.status?.state === "ready";

  it("is offered while the panel is empty and no agent runs; the replay fills the conversation, said once", async () => {
    const { store, acp, say } = await setup();
    expect(store.resumable).toEqual(
      expect.objectContaining({ title: "Clearer first steps in the guide", supports: "load" }),
    );
    await store.continueLast();
    await until(ready(store));
    expect(kinds(store.transcript)).toEqual(["you", "tool", "agent", "notice"]);
    expect(store.transcript[0]).toEqual(
      expect.objectContaining({
        kind: "you",
        text: "Make the first steps in the guide clearer.",
        passage: null,
      }),
    );
    // the copy of the earlier run is gone, the change it left with it
    expect(store.transcript.at(-1)).toEqual(
      expect.objectContaining({ kind: "notice", text: t("acp.restored.changesLost") }),
    );
    expect(say.mock.calls.filter(([text]) => text === t("acp.say.restored"))).toHaveLength(1);
    expect(store.resumable).toBeNull();
    expect(acp.prompts).toEqual([]);
  });

  it("the pieces of one replayed message make one line", () => {
    const store = new AcpStore();
    store.take({ type: "you", text: "Make it " });
    store.take({ type: "you", text: "shorter." });
    store.take({ type: "text", text: "Done." });
    store.take({ type: "you", text: "Thanks." });
    expect(
      store.transcript.map((e) => (e.kind === "notice" ? "" : "text" in e ? e.text : "")),
    ).toEqual(["Make it shorter.", "Done.", "Thanks."]);
  });

  it("after a new conversation the one just left is offered; on the same copy nothing is said lost", async () => {
    const { store } = await setup();
    store.draft = "Shorten the guide";
    await store.send();
    await until(() => store.transcript.some((e) => e.kind === "question"));
    await store.newConversation();
    await settle();
    expect(store.resumable).toEqual(
      expect.objectContaining({ title: "Shorten the guide", supports: "load" }),
    );
    await store.continueLast();
    await until(ready(store));
    expect(store.transcript[0]).toEqual(
      expect.objectContaining({ kind: "you", text: "Shorten the guide" }),
    );
    expect(store.transcript.some((e) => e.kind === "notice")).toBe(false);
  });

  it("an agent that only resumes: nothing replayed, the conversation says the agent remembers", async () => {
    const { store } = await setup({ agent: "codex-acp" });
    // the conversation kept is another agent's
    expect(store.resumable).toBeNull();
    store.draft = "Go";
    await store.send();
    await until(() => store.transcript.some((e) => e.kind === "question"));
    await store.newConversation();
    await settle();
    expect(store.resumable?.supports).toBe("resume");
    await store.continueLast();
    await until(ready(store));
    expect(store.transcript).toEqual([
      expect.objectContaining({ kind: "notice", text: t("acp.restored.notShown") }),
    ]);
  });

  it("a refusal is said and the offer follows what the backend holds", async () => {
    const { store, acp } = await setup();
    acp.saved = null;
    await store.continueLast();
    await settle();
    expect(store.transcript.at(-1)).toEqual(
      expect.objectContaining({ kind: "notice", text: t("acp.error.noSaved") }),
    );
    expect(store.status?.last).toBeNull();
  });

  it("a conversation the agent no longer has is said in words and no longer offered", async () => {
    const { store } = await setup();
    expect(store.status?.last).not.toBeNull();
    store.take({ type: "status", state: "error", error: { code: "sessionGone" } });
    expect(store.transcript.at(-1)).toEqual(
      expect.objectContaining({ kind: "notice", text: t("acp.fail.sessionGone") }),
    );
    expect(store.status?.last).toBeNull();
    // any other failure leaves the offer to the backend's next answer
    const other = await setup();
    other.store.take({ type: "status", state: "error", error: { code: "exited" } });
    expect(other.store.status?.last).not.toBeNull();
  });
});

describe("the choice of the agent", () => {
  it("none chosen: the picker shows and a message is refused; a choice is read back by name and starts next", async () => {
    const { store, acp } = await setup(null);
    expect(store.status?.chosen).toBe(false);
    expect(store.showsPicker).toBe(true);
    await store.loadAgents();
    if (store.catalog.status !== "ready") throw new Error("no list");
    expect(store.catalog.agents.map((a) => a.id)).toEqual(MOCK_AGENTS.map((a) => a.id));

    store.draft = "Hello";
    await store.send();
    expect(store.transcript.at(-1)).toEqual(
      expect.objectContaining({ text: t("acp.notSent", { reason: t("acp.error.noAgent") }) }),
    );

    expect(await store.choose({ agent: "codex-acp" })).toBe(true);
    expect(acp.choice).toEqual({ agent: "codex-acp" });
    expect(store.status).toEqual(
      expect.objectContaining({ chosen: true, agentName: "Codex", state: "idle" }),
    );
    expect(store.showsPicker).toBe(false);
    if (store.catalog.status !== "ready") throw new Error("no list");
    expect(store.catalog.agents.find((a) => a.chosen)?.id).toBe("codex-acp");

    await store.send();
    await until(() => store.transcript.some((e) => e.kind === "question"));
    expect(acp.prompts).toHaveLength(1);
  });

  it("Change shows the picker over the conversation until a choice | Cancel; a new agent leaves the lines to read", async () => {
    const { store } = await setup();
    store.draft = "Go";
    await store.send();
    await until(() => store.transcript.some((e) => e.kind === "question"));
    // the backend refuses a choice while the agent works
    expect(store.canChange).toBe(false);
    expect(await store.choose({ agent: "goose" })).toBe(false);
    expect(store.choiceRefusal).toBe(t("acp.error.busy"));
    await store.cancel();
    await settle();

    store.openPicker();
    expect(store.showsPicker).toBe(true);
    expect(store.choiceRefusal).toBeNull();
    store.closePicker();
    expect(store.showsPicker).toBe(false);

    store.openPicker();
    const before = store.transcript.length;
    expect(await store.choose({ command: '  "C:\\my tools\\my-agent.cmd" --acp ' })).toBe(true);
    await settle();
    expect(store.showsPicker).toBe(false);
    // an own command is named by the window, never by its program file
    expect(store.status?.chosen).toBe(true);
    expect(store.status?.agentName).toBeNull();
    expect(store.transcript.length).toBeGreaterThan(before);
    // a line between the conversations: the new agent does not know the one above
    expect(store.transcript.at(-1)).toEqual(
      expect.objectContaining({ kind: "divider", name: t("acp.ownCommand") }),
    );
    if (store.catalog.status !== "ready") throw new Error("no list");
    expect(store.catalog.agents.at(-1)).toEqual(
      expect.objectContaining({ id: "custom", how: "custom", chosen: true }),
    );
  });

  it("an empty own command and an agent no longer listed are refused in words", async () => {
    const { store } = await setup(null);
    expect(await store.choose({ command: "   " })).toBe(false);
    expect(store.choiceRefusal).toBe(t("acp.error.emptyCommand"));
    expect(await store.choose({ agent: "gone-agent" })).toBe(false);
    expect(store.choiceRefusal).toBe(t("acp.error.unknownAgent"));
    expect(store.status?.chosen).toBe(false);
  });

  it("an own command is previewed before it is kept: the program that would start, or why not, in words", async () => {
    const { store, acp } = await setup(null);
    const said = async (command: string) => {
      const resolved = await store.resolve(command);
      return resolved === null ? "failed" : previewText(resolved);
    };
    expect(await said('  "C:\\my tools\\my-agent.cmd" --acp ')).toBe(
      t("acp.preview.starts", { program: "C:\\my tools\\my-agent.cmd" }),
    );
    expect(await said("npx -y my-agent")).toBe(
      t("acp.preview.starts", { program: "C:\\Program Files\\nodejs\\npx.cmd" }),
    );
    expect(await said("missing-agent --acp")).toBe(t("acp.why.notFound"));
    expect(await said("tools/agent --acp")).toBe(t("acp.error.relativeProgram"));
    expect(await said("\\\\host\\share\\agent.exe --acp")).toBe(t("acp.error.networkProgram"));
    expect(await said('"my agent --acp')).toBe(t("acp.error.openQuote"));
    expect(await said('"" --acp')).toBe(t("acp.error.emptyCommand"));
    expect(acp.choice).toBeNull();
    acp.snap = true;
    expect(await said("my-agent")).toBe(t("acp.snap"));
  });

  it("a preview answered for another project is dropped", async () => {
    const { store, client, root } = await setup();
    const held = hold(client, "acp.resolve");
    const asked = store.resolve("my-agent");
    root.value = "C:/elsewhere";
    held.answer({ ok: true, value: { program: "C:\\x\\my-agent.exe", error: null } });
    expect(await asked).toBeNull();
  });

  it("How to install opens the agent's website through the app's link opening", async () => {
    const openWeb = vi.spyOn(documents, "openWeb").mockResolvedValue();
    const { store } = await setup(null);
    store.openWebsite("https://cline.bot/cli");
    expect(openWeb).toHaveBeenCalledWith("https://cline.bot/cli");
  });
});

describe("a sign-in the agent asks for", () => {
  it("shows its ways; Try again alone stays signed out, the agent's own way signs in and the reply goes on", async () => {
    const { store, acp } = await setup({ agent: "gemini" });
    store.draft = "Hello";
    await store.send();
    await until(() => store.status?.state === "needsLogin");
    expect(store.status?.methods).toEqual(MOCK_LOGIN_METHODS);
    expect(store.busy).toBe(false);

    await store.login(null);
    await until(
      () =>
        store.status?.state === "needsLogin" &&
        store.status.error?.code === MOCK_NOT_SIGNED_IN.code,
    );
    expect(store.transcript.at(-1)).toEqual(
      expect.objectContaining({ kind: "signIn", text: failureText(MOCK_NOT_SIGNED_IN) }),
    );

    const agentWay = MOCK_LOGIN_METHODS.find((m) => m.kind === "agent");
    if (agentWay === undefined) throw new Error("no agent method");
    await store.login(agentWay);
    await until(() => store.transcript.some((e) => e.kind === "question"));
    expect(acp.logins).toEqual([{}, { methodId: agentWay.id }]);
    expect(store.status?.state).toBe("working");
  });

  it("another agent chosen meanwhile: the old agent's sign-in news and card go", async () => {
    const { store } = await setup({ agent: "gemini" });
    store.draft = "Hello";
    await store.send();
    await until(() => store.status?.state === "needsLogin");
    await store.login(null);
    await until(() => store.transcript.some((e) => e.kind === "signIn"));
    expect(await store.choose({ agent: "codex-acp" })).toBe(true);
    expect(store.transcript.some((e) => e.kind === "signIn")).toBe(false);
    expect(store.transcript.map((e) => e.kind === "notice" && e.text)).not.toContain(
      failureText(MOCK_NOT_SIGNED_IN),
    );
    expect(store.status).toEqual(
      expect.objectContaining({ state: "idle", error: null, methods: [], agentName: "Codex" }),
    );
    // the conversation itself stays to read
    expect(store.transcript[0]).toEqual(expect.objectContaining({ kind: "you", text: "Hello" }));
  });

  it("a terminal | variable way only asks the agent again; a sign-in no longer waiting is said", async () => {
    const { store, acp } = await setup({ agent: "gemini" });
    const terminal = MOCK_LOGIN_METHODS.find((m) => m.kind === "terminal");
    if (terminal === undefined) throw new Error("no terminal method");
    await store.login(terminal);
    expect(store.transcript.at(-1)).toEqual(
      expect.objectContaining({ kind: "notice", text: t("acp.error.noLogin") }),
    );
    store.draft = "Hello";
    await store.send();
    await until(() => store.status?.state === "needsLogin");
    await store.login(terminal);
    // the person signed in outside the app: the call carries no method of the agent
    expect(acp.logins).toEqual([{}]);
  });
});

describe("the picker's list", () => {
  const entry = (
    id: string,
    how: AgentEntry["how"],
    needs: AgentEntry["needs"],
    description = "",
  ): AgentEntry => ({
    id,
    name: id.toUpperCase(),
    description,
    version: "1",
    how,
    needs,
    command: id,
    website: null,
    chosen: false,
  });

  const ids = (list: ReturnType<typeof pickerList>) => ({
    own: list.own.map((a) => a.id),
    popular: list.popular.map((a) => a.id),
    all: list.all.map((a) => a.id),
  });
  const own: AgentEntry = {
    ...entry("custom", "custom", null),
    name: "",
    command: '"C:\\a b\\node.exe" agent.mjs',
    chosen: true,
  };

  it("popular agents first in their own order, then all the others A-Z; the own command apart", () => {
    const agents = [
      entry("zed", "npx", null),
      entry("goose", "native", null, "Local coding agent"),
      entry("amp", "missing", "install", "Frontier coding agent"),
      entry("codex-acp", "missing", "node", "OpenAI coding assistant"),
      entry("cline", "missing", "node", "Coding agent CLI"),
      entry("claude-acp", "npx", null, "Anthropic's Claude"),
      own,
    ];
    expect(ids(pickerList(agents, ""))).toEqual({
      own: ["custom"],
      popular: ["claude-acp", "codex-acp", "goose"],
      all: ["amp", "cline", "zed"],
    });
  });

  it("finds every word in the name, id or description in every group; the own command by its words too", () => {
    const agents = [
      entry("amp", "missing", "install", "Frontier coding agent"),
      entry("claude-acp", "npx", null, "Anthropic's Claude"),
      entry("cline", "missing", "node", "Coding agent CLI"),
      entry("goose", "native", null, "Local coding agent"),
      own,
    ];
    expect(ids(pickerList(agents, " CODING  agent "))).toEqual({
      own: [],
      popular: ["goose"],
      all: ["amp", "cline"],
    });
    expect(ids(pickerList(agents, "cla"))).toEqual({ own: [], popular: ["claude-acp"], all: [] });
    expect(ids(pickerList(agents, t("acp.ownCommand")))).toEqual({
      own: ["custom"],
      popular: [],
      all: [],
    });
    expect(ids(pickerList(agents, "agent.mjs")).own).toEqual(["custom"]);
    expect(ids(pickerList(agents, "nothing"))).toEqual({ own: [], popular: [], all: [] });
  });

  it("describes a popular agent in PerMark's words, any other in the list's; finds both", () => {
    const claude = entry("claude-acp", "npx", null, "ACP wrapper for Anthropic's Claude");
    const amp = entry("amp", "missing", "install", "Frontier coding agent");
    expect(entryAbout(claude)).toBe(t("acp.about.claude-acp"));
    expect(entryAbout(amp)).toBe("Frontier coding agent");
    expect(entryAbout(own)).toBe("");
    expect(ids(pickerList([claude, amp], t("acp.about.claude-acp"))).popular).toEqual([
      "claude-acp",
    ]);
  });

  it("names the own command in words, a listed agent by its name", () => {
    expect(entryName(own)).toBe(t("acp.ownCommand"));
    expect(entryName(entry("goose", "native", null))).toBe("GOOSE");
  });

  it("says how each agent starts, else what it needs", () => {
    expect(howKey(entry("a", "native", null))).toBe("acp.how.native");
    expect(howKey(entry("a", "npx", null))).toBe("acp.how.npx");
    expect(howKey(entry("a", "uvx", null))).toBe("acp.how.uvx");
    expect(howKey(entry("a", "custom", null))).toBe("acp.how.custom");
    expect(howKey(entry("a", "custom", "install"))).toBe("acp.how.notFound");
    expect(howKey(entry("a", "missing", "node"))).toBe("acp.needs.node");
    expect(howKey(entry("a", "missing", "uv"))).toBe("acp.needs.uv");
    expect(howKey(entry("a", "missing", "install"))).toBe("acp.needs.install");
    expect(canStart(entry("a", "custom", "install"))).toBe(false);
    expect(canStart(entry("a", "custom", null))).toBe(true);
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
    expect(errorText(backend("unavailable", "noAgent"))).toBe(t("acp.error.noAgent"));
    expect(errorText(backend("unavailable", "noProfile"))).toBe(t("acp.error.noProfile"));
    expect(errorText(backend("invalidData", "unknownMethod"))).toBe(t("acp.error.unknownMethod"));
    expect(errorText(backend("invalidData", "openQuote"))).toBe(t("acp.error.openQuote"));
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

  it("words every failure code, adds the agent's own words, keeps its error output apart and capped", () => {
    const codes: AcpErrorCode[] = [
      "noAgent",
      "notFound",
      "noProfile",
      "copyFailed",
      "tooLarge",
      "projectsNotReady",
      "startFailed",
      "timedOut",
      "initFailed",
      "protocolVersion",
      "sessionFailed",
      "loginFailed",
      "stillNeedsLogin",
      "refused",
      "protocol",
      "exited",
      "sendFailed",
      "sessionGone",
      "optionRefused",
    ];
    for (const code of codes) expect(failureText({ code })).not.toBe(`acp.fail.${code}`);
    expect(new Set(codes.map((code) => failureText({ code }))).size).toBe(codes.length);
    expect(failureText({ code: "refused", message: "No." })).toBe(
      t("acp.fail.said", { reason: t("acp.fail.refused"), message: "No." }),
    );
    const store = new AcpStore();
    store.take({
      type: "status",
      state: "error",
      error: { code: "startFailed", stderr: `start${"x".repeat(9000)}end` },
    });
    const notice = store.transcript.at(-1);
    if (notice?.kind !== "notice") throw new Error("no notice");
    expect(notice.text).toBe(t("acp.fail.startFailed"));
    // the newest output counts: the end stays
    expect(notice.details?.endsWith("end")).toBe(true);
    expect(notice.details?.length).toBeLessThanOrEqual(4000);
  });

  it("words every refusal the plugin gives", () => {
    const details = [
      "busy",
      "emptyPrompt",
      "lineRange",
      "passageTooLarge",
      "noQuestion",
      "unsaved",
      "changed",
      "exists",
      "wholeFileOnly",
      "unknownHunk",
      "outdated",
      "noChange",
      "noWorkCopy",
      "noAgent",
      "noSaved",
      "unknownAgent",
      "emptyCommand",
      "openQuote",
      "networkProgram",
      "relativeProgram",
      "oneChoice",
      "noProfile",
      "noLogin",
      "unknownMethod",
      "unknownOption",
      "denied",
      "link",
      "linkInPath",
      "noUndo",
      "hiddenFromAgents",
      "snap",
      "insideProject",
      "scanCutOff",
      "tooManyFiles",
      "tooManyBytes",
      "projectsNotReady",
      "noText",
      "edits",
      "starting",
      "notRunning",
      "unknownSetting",
      "notOffered",
    ];
    for (const detail of details) {
      const error = backend("conflict", detail);
      expect(errorText(error), detail).not.toBe(apiErrorText(error));
    }
  });
});

describe("the conversation as the person follows it", () => {
  it("a question says the agent waits for an answer, and the window announces it", async () => {
    const { store, say } = await setup();
    store.draft = "Go";
    await store.send();
    await until(() => store.transcript.some((e) => e.kind === "question"));
    expect(store.asking).toBe(true);
    expect(say).toHaveBeenCalledWith(t("acp.say.working"));
    expect(say).toHaveBeenLastCalledWith(t("acp.say.waiting"));
    const question = store.transcript.find((e) => e.kind === "question");
    if (question?.kind !== "question" || question.options[0] === undefined) throw new Error("no");
    await store.answer(question.requestId, question.options[0]);
    expect(store.asking).toBe(false);
    await until(() => store.status?.state === "ready");
    // the end says itself with the changes ready to review, in one line
    expect(say).toHaveBeenLastCalledWith(
      t("acp.say.withChanges", { said: t("acp.say.done"), changes: tn1() }),
    );
  });

  it("Stop: unfinished tool lines show as stopped, the end says the changes made stay listed", () => {
    const store = new AcpStore(undefined, undefined, { say: () => {} });
    store.take({ type: "tool", id: "a", title: "Read", kind: "read", status: "completed" });
    store.take({ type: "tool", id: "b", title: "Edit", kind: "edit", status: "in_progress" });
    store.take({ type: "tool", id: "c", title: "Run", kind: "execute", status: "pending" });
    store.take({ type: "turnEnd", stopReason: "cancelled", changed: 1 });
    expect(store.transcript.map((e) => e.kind === "tool" && e.status)).toEqual([
      "completed",
      "stopped",
      "stopped",
      false,
    ]);
    expect(store.transcript.at(-1)).toEqual(
      expect.objectContaining({ kind: "notice", text: t("acp.stoppedChanges") }),
    );
    store.take({ type: "turnEnd", stopReason: "cancelled", changed: 0 });
    expect(store.transcript.at(-1)).toEqual(
      expect.objectContaining({ kind: "notice", text: t("acp.stopped") }),
    );
  });

  it("a card still open at the turn's end | a stop closes; an answer leaves an earlier card of the same id", async () => {
    const store = new AcpStore(undefined, undefined, { say: () => {} });
    const allow = { id: "yes", name: "Allow", kind: "allow_once" } as const;
    const ask = (requestId: string): void =>
      store.take({
        type: "permission",
        requestId,
        title: "Run",
        kind: "execute",
        reach: "outside",
        detail: "x",
        options: [allow],
      });
    const answers = (): unknown[] =>
      store.transcript.flatMap((e) => (e.kind === "question" ? [e.answer] : []));
    // the backend answered it while its card was still on the way: no PermissionDone comes after
    ask("0");
    store.take({ type: "turnEnd", stopReason: "end_turn", changed: 0 });
    expect(answers()).toEqual([{ status: "closed" }]);
    // a restarted agent numbers its questions from the start again
    ask("0");
    const answering = store.answer("0", allow);
    // the answer is said as its button said it
    expect(answers()).toEqual([
      { status: "closed" },
      { status: "answered", option: t("acp.option.allowOnce") },
    ]);
    await answering;
    ask("1");
    store.take({ type: "status", state: "ready", error: null });
    expect(answers().at(-1)).toEqual({ status: "closed" });
  });

  it("an accept | reject leaves one notice per file whose Undo takes it back", async () => {
    const { store, acp, notifyKeyed } = await setup();
    acp.changes = [
      {
        path: "docs/guide.md",
        kind: "modified",
        hunks: [
          {
            key: "k7",
            oldStart: 7,
            oldLines: ["a"],
            newLines: ["b"],
            truncated: false,
            contextBefore: null,
            contextAfter: null,
          },
        ],
      },
    ];
    await store.refreshChanges();
    await store.decide("accept", "docs/guide.md", ["k7"]);
    expect(store.changes).toEqual([]);
    expect(notifyKeyed).toHaveBeenCalledTimes(1);
    const [key, text, undo] = notifyKeyed.mock.calls[0] as [
      string,
      string,
      { label: string; run: () => Promise<void> },
    ];
    expect(key).toContain("docs/guide.md");
    expect(text).toBe(t("acp.done.accepted", { path: "docs/guide.md" }));
    expect(undo.label).toBe(t("fileOps.undo"));
    await undo.run();
    expect(acp.calls).toContain("undo");
    expect(store.changes[0]?.hunks[0]?.key).toBe("k7");
    await store.decide("reject", "docs/guide.md", null);
    expect(notifyKeyed).toHaveBeenLastCalledWith(
      key,
      t("acp.done.rejected", { path: "docs/guide.md" }),
      expect.objectContaining({ label: t("fileOps.undo") }),
    );
    // nothing left to undo: said beside the list
    await undo.run();
    await undo.run();
    expect(store.refusal?.text).toBe(t("acp.error.noUndo"));
  });

  it("keeps the newest lines past the cap and says earlier ones are hidden", () => {
    const store = new AcpStore(undefined, undefined, { say: () => {} });
    for (let i = 0; i < MAX_ENTRIES + 20; i++)
      store.take({
        type: "tool",
        id: `t${String(i)}`,
        title: "Read",
        kind: "read",
        status: "completed",
      });
    expect(store.transcript).toHaveLength(MAX_ENTRIES);
    expect(store.hidden).toBe(20);
    expect(store.transcript.at(-1)).toEqual(
      expect.objectContaining({ tool: `t${String(MAX_ENTRIES + 19)}` }),
    );
  });

  it("another agent chosen in any window draws a line in the conversation", async () => {
    const { store, acp } = await setup();
    store.draft = "Go";
    await store.send();
    await until(() => store.transcript.some((e) => e.kind === "question"));
    await store.cancel();
    await settle();
    // a choice made in another window: the agent stops (idle), the name is read again
    acp.choice = { agent: "goose" };
    store.take({ type: "status", state: "idle", error: null });
    await settle();
    expect(store.transcript.at(-1)).toEqual(
      expect.objectContaining({ kind: "divider", name: "goose" }),
    );
  });

  it("files left out of the copy for their size are read again at the turn's end", async () => {
    const { store, acp } = await setup();
    expect(store.status?.skippedFiles).toBe(0);
    acp.skipped = 3;
    store.draft = "Go";
    await store.send();
    await until(() => store.transcript.some((e) => e.kind === "question"));
    await store.cancel();
    await until(() => store.status?.skippedFiles === 3);
  });

  it("inside a Snap the picker stands in, whatever was chosen", async () => {
    const { store, acp } = await setup();
    expect(store.showsPicker).toBe(false);
    acp.snap = true;
    store.take({ type: "status", state: "idle", error: null });
    await until(() => store.status?.needs === "snap");
    expect(store.showsPicker).toBe(true);
    expect(await store.choose({ agent: "goose" })).toBe(false);
    expect(store.choiceRefusal).toBe(t("acp.snap"));
  });
});

describe("the agent's settings", () => {
  it("show once the agent runs; a change reaches it and comes back; a new conversation empties them", async () => {
    const { store, acp, client, root } = await setup();
    expect(store.options).toEqual([]);
    expect(store.canSetOptions).toBe(false);
    store.draft = "Hello";
    await store.send();
    await until(() => store.status?.state === "ready" || store.asking);
    expect(store.options.map((o) => o.id)).toEqual(["model", "effort", "fast"]);
    expect(store.canSetOptions).toBe(true);
    expect(await store.setOption("model", "opus")).toBe(true);
    expect(await store.setOption("fast", true)).toBe(true);
    expect(acp.optionSets).toEqual([
      { id: "model", value: "opus" },
      { id: "fast", value: true },
    ]);
    const model = store.options.find((o) => o.id === "model");
    expect(model?.kind.type === "select" && model.kind.current).toBe("opus");
    const fast = store.options.find((o) => o.id === "fast");
    expect(fast?.kind).toEqual({ type: "toggle", on: true });
    // a window that reads the status later shows them too
    const later = new AcpStore(client, () => root.value, {
      say: vi.fn(),
      notices: { notifyKeyed: vi.fn() },
    });
    stops.push(later.start());
    await settle();
    expect(later.options).toEqual(store.options);
    await store.newConversation();
    await settle();
    expect(store.options).toEqual([]);
    // the next start brings the picks back
    store.draft = "Again";
    await store.send();
    await until(() => store.options.length > 0);
    const again = store.options.find((o) => o.id === "model");
    expect(again?.kind.type === "select" && again.kind.current).toBe("opus");
  });

  it("a change refused before it reaches the agent is said in words; the agent's own refusal with its words", async () => {
    const { store, acp } = await setup();
    expect(await store.setOption("model", "opus")).toBe(false);
    expect(store.transcript.at(-1)).toEqual(
      expect.objectContaining({ kind: "notice", text: t("acp.error.notRunning") }),
    );
    store.draft = "Hello";
    await store.send();
    await until(() => store.options.length > 0 && store.canSetOptions);
    expect(await store.setOption("model", "gpt")).toBe(false);
    expect(store.transcript.at(-1)).toEqual(
      expect.objectContaining({ kind: "notice", text: t("acp.error.settingGone") }),
    );
    acp.refuseOption = "Not on your plan";
    const before = store.options;
    expect(await store.setOption("model", "haiku")).toBe(true);
    expect(store.options).toEqual(before);
    expect(store.transcript.at(-1)).toEqual(
      expect.objectContaining({
        kind: "notice",
        text: t("acp.fail.said", {
          reason: t("acp.fail.optionRefused"),
          message: "Not on your plan",
        }),
      }),
    );
  });

  it("are dropped when the agent stops | fails, and replaced whole by the agent's news", () => {
    const store = new AcpStore();
    store.take({ type: "status", state: "ready", error: null });
    const options = [
      {
        id: "effort",
        name: "Effort",
        description: null,
        category: "thoughtLevel" as const,
        kind: { type: "toggle" as const, on: true },
      },
    ];
    store.take({ type: "options", options });
    expect(store.options).toEqual(options);
    store.take({ type: "status", state: "error", error: { code: "exited" } });
    expect(store.options).toEqual([]);
    store.take({ type: "options", options });
    store.take({ type: "status", state: "working", error: null });
    expect(store.options).toEqual(options);
    store.take({ type: "status", state: "idle", error: null });
    expect(store.options).toEqual([]);
  });

  it("the model is labelled in the window's words, any other setting by the agent's name for it", () => {
    const kind = { type: "toggle" as const, on: false };
    expect(
      configLabel({ id: "m", name: "Modelo", description: null, category: "model", kind }),
    ).toBe(t("acp.option.model"));
    expect(
      configLabel({ id: "e", name: "Effort", description: null, category: "thoughtLevel", kind }),
    ).toBe("Effort");
  });
});

describe("another project shown meanwhile", () => {
  it("its agent's news never reach this one; the conversation starts afresh", async () => {
    const { store, root, acp } = await setup();
    store.draft = "Go";
    await store.send();
    await until(() => store.transcript.some((e) => e.kind === "agent"));
    root.value = "F:/elsewhere";
    flushSync();
    expect(store.transcript).toEqual([]);
    // the mock's news name the first project: dropped
    acp.stepMs = 1;
    await settle();
    expect(store.transcript).toEqual([]);
    expect(store.sending).toBe(false);
  });

  it("a refusal that comes back after the switch puts nothing in the new conversation", async () => {
    const { store, root } = await setup(null);
    store.draft = "Hello";
    store.passage = { path: "a.md", fromLine: 1, toLine: 1, text: "x" };
    const sending = store.send();
    root.value = "F:/elsewhere";
    flushSync();
    await sending;
    expect(store.transcript).toEqual([]);
    expect(store.draft).toBe("");
    expect(store.passage).toBeNull();
  });

  it("a late list of changes | of agents is dropped", async () => {
    const { store, root, client } = await setup();
    const changes = hold(client, "acp.changes");
    const reading = store.refreshChanges();
    root.value = "F:/elsewhere";
    flushSync();
    const late: ChangeList = {
      files: [{ path: "x.md", kind: "added", hunks: [] }],
      more: 0,
    };
    changes.answer({ ok: true, value: late });
    await reading;
    expect(store.changes).toEqual([]);
    vi.restoreAllMocks();

    const agents = hold(client, "acp.agents");
    const loading = store.loadAgents();
    root.value = MOCK_FOLDER;
    flushSync();
    agents.answer({ ok: true, value: [] });
    await loading;
    expect(store.catalog.status).not.toBe("ready");
  });

  it("a message rendered for the project before is dropped", async () => {
    const { store, root, client } = await setup();
    const render = hold(client, "acp.render");
    const rendering = store.render("# Late");
    root.value = "F:/elsewhere";
    flushSync();
    render.answer({ ok: true, value: { blocks: [] } });
    expect(await rendering).toBeNull();
  });

  it("a list that cannot be read says why and reads again on Try again", async () => {
    const { store, client } = await setup(null);
    const agents = hold(client, "acp.agents");
    const loading = store.loadAgents();
    agents.answer({
      ok: false,
      error: {
        kind: "backend",
        command: "plugin_call",
        error: { kind: "io", subject: null, detail: null },
      },
    });
    await loading;
    expect(store.catalog.status).toBe("failed");
    vi.restoreAllMocks();
    await store.loadAgents();
    expect(store.catalog.status).toBe("ready");
  });
});

describe("the place of the panel", () => {
  it("first shown: a tab of the outline's group, never a cramped group of its own", () => {
    const arr = defaultArrangement();
    const placed = firstPlace(arr);
    expect(placed).toEqual(intoGroup(arr, PANEL, "right", 0, 1));
    expect(placed?.right[0]?.views).toEqual([OUTLINE, PANEL]);
    // arranged by the person before (any place): left as it is
    expect(firstPlace(placed ?? arr)).toBeNull();
    // the outline moved elsewhere: the default place stands
    expect(firstPlace({ ...arr, right: [], left: [{ views: [OUTLINE], share: 1 }] })).toBeNull();
  });
});

describe("the attached passage as the selection moves", () => {
  it("measures the lines at most once per moment while a selection is dragged", () => {
    vi.useFakeTimers();
    try {
      vi.spyOn(documents, "path", "get").mockReturnValue("docs/a.md");
      vi.spyOn(editing, "path", "get").mockReturnValue("docs/a.md");
      editing.mode = "source";
      const current = vi.spyOn(editing, "current").mockReturnValue("alpha\nbeta");
      vi.spyOn(editing, "scope").mockReturnValue({ from: 0, to: 5 });
      editing.selection = "alpha";
      const store = new AcpStore(undefined, () => null);
      stops.push(store.start());
      flushSync();
      for (let i = 0; i < 50; i++) {
        document.dispatchEvent(new Event("selectionchange"));
        vi.advanceTimersByTime(5);
      }
      vi.advanceTimersByTime(500);
      expect(current.mock.calls.length).toBeLessThanOrEqual(5);
      expect(store.passage?.text).toBe("alpha");
    } finally {
      vi.useRealTimers();
    }
  });
});

describe("the mock follows the plugin's checks", () => {
  it("a passage alone is a message; a bad range | path is refused as the plugin does", async () => {
    const { api } = await setup();
    const call = (args: object) => api.pluginCall("acp.prompt", args as never);
    const selection = { path: "a.md", fromLine: 2, toLine: 3, text: "x" };
    expect((await call({ text: "", selection })).ok).toBe(true);
    api.probe.plugins.acp.stopAll();
    await api.pluginCall("acp.cancel", {});
    const refused = async (args: object) => {
      const result = await call(args);
      return result.ok || result.error.kind !== "backend" ? null : result.error.error;
    };
    expect(await refused({ text: "" })).toEqual(expect.objectContaining({ detail: "emptyPrompt" }));
    expect(await refused({ text: "x", selection: { ...selection, fromLine: 0 } })).toEqual(
      expect.objectContaining({ detail: "lineRange" }),
    );
    // the plugin's cap counts bytes: 256 KiB of two-byte characters is past it
    expect(
      await refused({ text: "x", selection: { ...selection, text: "é".repeat(128 * 1024 + 1) } }),
    ).toEqual(expect.objectContaining({ kind: "tooLarge", detail: "passageTooLarge" }));
    expect(
      (await call({ text: "x", selection: { ...selection, text: "x".repeat(256 * 1024) } })).ok,
    ).toBe(true);
    api.probe.plugins.acp.stopAll();
    await api.pluginCall("acp.cancel", {});
    expect(await refused({ text: "x", selection: { ...selection, path: "../a.md" } })).toEqual(
      expect.objectContaining({ kind: "invalidPath" }),
    );
    expect(await refused({ text: "x", document: "" })).toEqual(
      expect.objectContaining({ kind: "invalidPath" }),
    );
  });

  it("a message to render is capped in bytes as the plugin caps it", async () => {
    const { api } = await setup();
    const render = (text: string) => api.pluginCall("acp.render", { text });
    const refused = await render("é".repeat(128 * 1024 + 1));
    expect(!refused.ok && refused.error.kind === "backend" && refused.error.error).toEqual(
      expect.objectContaining({ kind: "tooLarge", detail: "replyTooLarge" }),
    );
    expect((await render("x".repeat(256 * 1024))).ok).toBe(true);
  });

  it("a file taken only as a whole refuses part of its hunks; a link is never accepted", async () => {
    const { api, acp } = await setup();
    acp.changes = [
      {
        path: "new.md",
        kind: "added",
        hunks: [
          {
            key: "n1",
            oldStart: 1,
            oldLines: [],
            newLines: ["a"],
            truncated: false,
            contextBefore: null,
            contextAfter: null,
          },
          {
            key: "n2",
            oldStart: 1,
            oldLines: [],
            newLines: ["b"],
            truncated: false,
            contextBefore: null,
            contextAfter: null,
          },
        ],
      },
      { path: "both.md", kind: "conflict", hunks: [] },
      { path: "ln", kind: "link", hunks: [] },
    ];
    const detail = async (
      method: "acp.accept" | "acp.reject",
      path: string,
      hunks: string[] | null,
    ) => {
      const result = await api.pluginCall(method, { path, hunks });
      return result.ok || result.error.kind !== "backend" ? null : result.error.error.detail;
    };
    expect(await detail("acp.accept", "new.md", ["n1"])).toBe("wholeFileOnly");
    expect(await detail("acp.accept", "both.md", ["x"])).toBe("changed");
    expect(await detail("acp.accept", "ln", null)).toBe("link");
    expect(await detail("acp.reject", "ln", null)).toBeNull();
    const own = await api.pluginCall("acp.choose", { command: "tools/agent --acp" });
    expect(!own.ok && own.error.kind === "backend" && own.error.error.detail).toBe(
      "relativeProgram",
    );
  });

  it("a preview refuses exactly what a choice refuses, and keeps nothing", async () => {
    const { api, acp } = await setup(null);
    for (const command of [
      "",
      "  ",
      '"" --acp',
      '"my agent',
      "./agent",
      "bin\\agent.exe",
      "\\\\host\\share\\agent.exe",
      "//host/share/agent",
      '"\\\\?\\UNC\\host\\share\\my agent.exe" --acp',
      "my-agent",
    ]) {
      const preview = await api.pluginCall("acp.resolve", { command });
      if (!preview.ok) throw new Error("no preview");
      expect(acp.choice, command).toBeNull();
      const choice = await api.pluginCall("acp.choose", { command });
      const refusal =
        !choice.ok && choice.error.kind === "backend" ? choice.error.error.detail : null;
      expect(preview.value.error, command).toBe(refusal);
      acp.choice = null;
    }
  });
});

/** What the end of the mock's turn says of its one changed file. */
function tn1(): string {
  return t("acp.say.changes.one", { n: 1 });
}

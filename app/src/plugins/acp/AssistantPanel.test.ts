// @vitest-environment jsdom
// The Assistant panel: the agent's text as plain text (markup never becomes elements), question cards that answer
// with the agent's options, Enter | Shift+Enter | an IME's Enter in the message box, Send while the agent works, the
// passage chip, and hunks accepted | rejected one by one (files taken only as a whole show no hunk buttons); the
// picker (no agent chosen | Change): search, a chip per state, How to install, Choose only for an agent that can
// start, an own command; the sign-in card (the agent's own way, a command to copy, variables, Try again); where the
// keyboard goes (a question, a sign-in, Stop, a decision), the bar to the changes, Open, the caps, the Snap line; the
// agent's settings under its name (labelled controls, off while it starts, a refused change shows what holds).
import "../../core/testing/media";
import { flushSync, mount, tick, unmount } from "svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { MOCK_AGENTS, MOCK_LOGIN_METHODS, MOCK_OPTIONS } from "../../api/mockAcp";
import { documents } from "../../core/doc/document.svelte";
import { t, tn } from "../../core/i18n";
import AssistantPanel, { OPTION_SETTLE_MS } from "./AssistantPanel.svelte";
import { acp, PANEL_MARK } from "./store.svelte";

let component: Record<string, unknown> | null = null;
let root: HTMLElement;

function open(): void {
  component = mount(AssistantPanel, { target: root, props: {} });
  flushSync();
}

function buttonNamed(name: string): HTMLButtonElement {
  const found = [...root.querySelectorAll("button")].find((b) => b.textContent?.trim() === name);
  if (!(found instanceof HTMLButtonElement)) throw new Error(`no button ${name}`);
  return found;
}

/** Lets the awaited calls of a click end and the panel draw again. */
async function settle(): Promise<void> {
  for (let i = 0; i < 3; i++) {
    await new Promise((resolve) => setTimeout(resolve, 0));
    flushSync();
  }
  await tick();
}

function box(): HTMLTextAreaElement {
  const found = root.querySelector("textarea");
  if (found === null) throw new Error("no message box");
  return found;
}

function labelled(label: string): HTMLButtonElement {
  const found = root.querySelector(`button[aria-label="${label}"]`);
  if (!(found instanceof HTMLButtonElement)) throw new Error(`no button labelled ${label}`);
  return found;
}

/** No agent chosen yet; the list as the mock computer has it. */
function firstRun(): void {
  acp.status = {
    state: "idle",
    agent: "",
    agentName: null,
    chosen: false,
    error: null,
    methods: [],
    skippedFiles: 0,
    last: null,
    options: [],
  };
  acp.catalog = { status: "ready", agents: MOCK_AGENTS.map((a) => ({ ...a, chosen: false })) };
}

beforeEach(() => {
  vi.spyOn(acp, "refreshChanges").mockResolvedValue();
  vi.spyOn(acp, "loadAgents").mockResolvedValue();
  // a whole message stays plain text unless a test renders it
  vi.spyOn(acp, "render").mockResolvedValue(null);
  acp.clear();
  acp.status = {
    state: "ready",
    agent: "npx -y @agentclientprotocol/claude-agent-acp@0.108.0",
    agentName: "Claude Agent",
    chosen: true,
    error: null,
    methods: [],
    skippedFiles: 0,
    last: null,
    options: [],
  };
  root = document.createElement("div");
  document.body.append(root);
});

afterEach(() => {
  if (component) unmount(component);
  component = null;
  root.remove();
  acp.clear();
  vi.restoreAllMocks();
});

describe("the Assistant panel", () => {
  it("explains itself while the conversation is empty and names the agent", () => {
    open();
    expect(root.textContent).toContain(t("acp.empty"));
    expect(root.querySelector(".agent")?.textContent).toContain("Claude Agent");
    expect(root.querySelector(`[${PANEL_MARK}]`)).not.toBeNull();
    expect(root.querySelector("[data-view-header]")).not.toBeNull();
  });

  it("shows the agent's text as plain text while it comes", () => {
    acp.take({ type: "status", state: "working", error: null });
    acp.take({ type: "text", text: "<b>bold</b> <img src=x onerror=alert(1)>" });
    open();
    const reply = root.querySelector(".agent-text .text");
    expect(reply?.textContent).toBe("<b>bold</b> <img src=x onerror=alert(1)>");
    expect(reply?.querySelector("b, img")).toBeNull();
  });

  it("renders a message once another line follows it or the turn ended", async () => {
    const render = vi.spyOn(acp, "render").mockImplementation((text) =>
      Promise.resolve([
        {
          id: "b0",
          kind: "p",
          lineStart: 1,
          lineEnd: 1,
          html: `<p data-b="b0"><em>${text}</em></p>`,
        },
      ]),
    );
    acp.take({ type: "status", state: "working", error: null });
    acp.take({ type: "text", text: "first" });
    open();
    await settle();
    expect(render).not.toHaveBeenCalled();
    acp.take({ type: "tool", id: "t1", title: "Read a.md", kind: "read", status: "pending" });
    acp.take({ type: "text", text: "second" });
    await settle();
    expect(render.mock.calls).toEqual([["first"]]);
    expect(root.querySelector(".agent-text .md-body em")?.textContent).toBe("first");
    acp.take({ type: "status", state: "ready", error: null });
    await settle();
    expect(render.mock.calls).toEqual([["first"], ["second"]]);
    expect([...root.querySelectorAll(".agent-text em")].map((e) => e.textContent)).toEqual([
      "first",
      "second",
    ]);
  });

  it("a question answers with the option the person picks, then says what was answered", () => {
    const answer = vi.spyOn(acp, "answer").mockResolvedValue();
    acp.take({
      type: "permission",
      requestId: "7",
      title: "Run tests",
      kind: "execute",
      reach: "outside",
      detail: "npm test",
      options: [
        { id: "yes", name: "Allow", kind: "allow_once" },
        { id: "no", name: "Reject", kind: "reject_once" },
      ],
    });
    open();
    expect(root.textContent).toContain("npm test");
    // what Allow reaches is said beside the buttons: the person's computer, not the copy
    expect(root.textContent).toContain(t("acp.questionHint"));
    buttonNamed(t("acp.option.allowOnce")).click();
    expect(answer).toHaveBeenCalledWith("7", { id: "yes", name: "Allow", kind: "allow_once" });
    acp.take({ type: "permissionDone", requestId: "7" });
    flushSync();
    expect(root.textContent).toContain(t("acp.questionClosed"));
    expect(root.textContent).not.toContain(t("acp.questionHint"));
  });

  it("a question's answers say what they do (the agent's words as a tooltip); 'always' answers are left out, with why", () => {
    const answer = vi.spyOn(acp, "answer").mockResolvedValue();
    const once = { id: "allow-once", name: "Yes", kind: "allow_once" } as const;
    acp.take({
      type: "permission",
      requestId: "1:7",
      title: "Run tests",
      kind: "execute",
      reach: "outside",
      detail: "npm test",
      options: [
        once,
        { id: "always", name: "Yes, and don't ask again", kind: "allow_always" },
        { id: "reject", name: "No", kind: "reject_once" },
        { id: "never", name: "No, never", kind: "reject_always" },
      ],
    });
    open();
    const card = root.querySelector(".question");
    const shown = [...(card?.querySelectorAll(".options button") ?? [])].map((b) => [
      b.textContent?.trim(),
      b.getAttribute("title"),
    ]);
    expect(shown).toEqual([
      [t("acp.option.allowOnce"), "Yes"],
      [t("acp.option.rejectOnce"), "No"],
    ]);
    expect(card?.textContent).toContain(t("acp.alwaysHidden"));
    buttonNamed(t("acp.option.allowOnce")).click();
    expect(answer).toHaveBeenCalledWith("1:7", once);
  });

  it("the card warns of what allowing reaches: the computer, the agent's own settings, or nothing past the copy", () => {
    const allow = { id: "yes", name: "Yes", kind: "allow_once" } as const;
    const ask = (requestId: string, reach: "outside" | "agentSettings" | "copy"): void =>
      acp.take({
        type: "permission",
        requestId,
        title: "Do it",
        kind: reach === "outside" ? "execute" : "edit",
        reach,
        detail: "x",
        options: [allow],
      });
    ask("1", "outside");
    ask("2", "agentSettings");
    ask("3", "copy");
    open();
    const [outside, settings, copy] = [...root.querySelectorAll(".question")].map(
      (card) => card.textContent ?? "",
    );
    expect(outside).toContain(t("acp.questionHint"));
    expect(outside).not.toContain(t("acp.questionSettings"));
    expect(settings).toContain(t("acp.questionSettings"));
    expect(settings).not.toContain(t("acp.questionHint"));
    expect(copy).not.toContain(t("acp.questionHint"));
    expect(copy).not.toContain(t("acp.questionSettings"));
  });

  it("the card names the kind of action and shows the change it would write; a card with no details says so", () => {
    const allow = { id: "yes", name: "Yes", kind: "allow_once" } as const;
    acp.take({
      type: "permission",
      requestId: "1",
      title: "Write settings",
      kind: "edit",
      reach: "agentSettings",
      detail: ".claude/settings.json",
      diff: {
        path: ".claude/settings.json",
        oldLines: ["{}"],
        newLines: ["{", '  "allow": ["Bash(*)"]', "}"],
        truncated: true,
      },
      summary: "Allow every command",
      options: [allow],
    });
    acp.take({
      type: "permission",
      requestId: "2",
      title: "Read notes",
      kind: "execute",
      reach: "outside",
      detail: "",
      options: [allow],
    });
    open();
    const [edit, bare] = root.querySelectorAll(".question");
    const rows = (kind: string): (string | null | undefined)[] =>
      [...(edit?.querySelectorAll(`.row.${kind} .text`) ?? [])].map((row) => row.textContent);
    expect(edit?.querySelector(".ask-kind")?.textContent).toBe(t("acp.toolKind.edit"));
    expect(rows("del")).toEqual(["{}"]);
    expect(rows("add")).toEqual(["{", '  "allow": ["Bash(*)"]', "}"]);
    expect(edit?.textContent).toContain(t("acp.diffCut"));
    expect(edit?.textContent).toContain("Allow every command");
    expect(edit?.textContent).not.toContain(t("acp.noDetails"));
    // a command titled like a read: its kind is said, and that the agent told nothing more
    expect(bare?.querySelector(".ask-kind")?.textContent).toBe(t("acp.toolKind.execute"));
    expect(bare?.textContent).toContain(t("acp.noDetails"));
  });

  it("Escape in the message box stops the agent while it works, as the hint says; on a card it answers nothing", () => {
    const cancel = vi.spyOn(acp, "cancel").mockResolvedValue();
    const answer = vi.spyOn(acp, "answer").mockResolvedValue();
    open();
    const escape = (isComposing = false): KeyboardEvent =>
      new KeyboardEvent("keydown", { key: "Escape", isComposing, cancelable: true, bubbles: true });
    const hint = (): string =>
      document.getElementById(box().getAttribute("aria-describedby") ?? "")?.textContent ?? "";
    box().dispatchEvent(escape());
    expect(cancel).not.toHaveBeenCalled();
    expect(hint()).not.toContain(t("acp.stopHint"));
    acp.take({ type: "status", state: "working", error: null });
    flushSync();
    expect(hint()).toContain(t("acp.stopHint"));
    // an IME's Escape ends its composition
    box().dispatchEvent(escape(true));
    expect(cancel).not.toHaveBeenCalled();
    const stopKey = escape();
    box().dispatchEvent(stopKey);
    expect(cancel).toHaveBeenCalledTimes(1);
    expect(stopKey.defaultPrevented).toBe(true);
    acp.take({
      type: "permission",
      requestId: "9",
      title: "Run",
      kind: "execute",
      reach: "outside",
      detail: "x",
      options: [{ id: "yes", name: "Yes", kind: "allow_once" }],
    });
    flushSync();
    const card = root.querySelector<HTMLElement>(".question");
    card?.dispatchEvent(escape());
    card?.querySelector("button")?.dispatchEvent(escape());
    expect(answer).not.toHaveBeenCalled();
    expect(cancel).toHaveBeenCalledTimes(1);
  });

  it("Enter sends; Shift+Enter and an IME's Enter do not", () => {
    const send = vi.spyOn(acp, "send").mockResolvedValue();
    open();
    acp.draft = "Hello";
    flushSync();
    box().dispatchEvent(
      new KeyboardEvent("keydown", { key: "Enter", shiftKey: true, bubbles: true }),
    );
    box().dispatchEvent(
      new KeyboardEvent("keydown", { key: "Enter", isComposing: true, bubbles: true }),
    );
    expect(send).not.toHaveBeenCalled();
    const enter = new KeyboardEvent("keydown", { key: "Enter", cancelable: true, bubbles: true });
    box().dispatchEvent(enter);
    expect(send).toHaveBeenCalledTimes(1);
    expect(enter.defaultPrevented).toBe(true);
  });

  it("Send turns into Stop in its place while the agent works; Stop gives the keyboard to the message box", async () => {
    const cancel = vi.spyOn(acp, "cancel").mockResolvedValue();
    open();
    acp.draft = "Hello";
    flushSync();
    const send = buttonNamed(t("acp.send"));
    expect(send.disabled).toBe(false);
    acp.take({ type: "status", state: "working", error: null });
    flushSync();
    // the same button, now Stop: nothing around it moves
    expect(buttonNamed(t("acp.stop"))).toBe(send);
    expect(root.querySelector("header")?.textContent).not.toContain(t("acp.stop"));
    send.focus();
    send.click();
    expect(cancel).toHaveBeenCalled();
    await settle();
    expect(document.activeElement).toBe(box());
  });

  it("a question waits: the status says so, its card takes the keyboard (never an answer), then the message box does", async () => {
    vi.spyOn(acp, "answer").mockResolvedValue();
    open();
    acp.take({ type: "status", state: "working", error: null });
    flushSync();
    buttonNamed(t("acp.stop")).focus();
    acp.take({
      type: "permission",
      requestId: "7",
      title: "Run tests",
      kind: "execute",
      reach: "outside",
      detail: "npm test",
      options: [
        { id: "no", name: "Reject", kind: "reject_once" },
        { id: "yes", name: "Allow", kind: "allow_once" },
      ],
    });
    await settle();
    expect(root.querySelector(".line .state")?.textContent).toBe(t("acp.state.waiting"));
    // a key pressed on the way never answers: the card is announced, Tab reaches its answers
    const card = root.querySelector<HTMLElement>(".question");
    expect(document.activeElement).toBe(card);
    expect(card?.getAttribute("aria-labelledby")).toBe(card?.querySelector(".ask")?.id);
    buttonNamed(t("acp.option.allowOnce")).click();
    await settle();
    expect(document.activeElement).toBe(box());
  });

  it("a question never takes the keyboard from the message box, even an empty one", async () => {
    open();
    flushSync();
    box().focus();
    acp.take({
      type: "permission",
      requestId: "8",
      title: "Run",
      kind: "execute",
      reach: "outside",
      detail: "x",
      options: [{ id: "yes", name: "Allow", kind: "allow_once" }],
    });
    await settle();
    expect(document.activeElement).toBe(box());
  });

  it("a sign-in needed: the card's title takes the keyboard", async () => {
    open();
    box().focus();
    acp.take({
      type: "status",
      state: "needsLogin",
      error: null,
      methods: [...MOCK_LOGIN_METHODS],
    });
    await settle();
    expect(document.activeElement?.textContent?.trim()).toBe(
      t("acp.login.title", { name: "Claude Agent" }),
    );
  });

  it("the conversation scrolls with the keyboard and is no live region; one Thinking at its end", () => {
    open();
    const scroller = root.querySelector(".scroll");
    expect(scroller?.getAttribute("tabindex")).toBe("0");
    expect(scroller?.getAttribute("aria-label")).toBe(t("acp.conversation"));
    expect(root.querySelector("[aria-live]")).toBeNull();
    acp.take({ type: "thinking" });
    acp.take({ type: "thinking" });
    flushSync();
    expect(root.querySelectorAll(".thinking")).toHaveLength(1);
    acp.take({ type: "text", text: "Done" });
    flushSync();
    expect(root.querySelector(".thinking")).toBeNull();
  });

  it("the empty conversation offers example requests that fill the message box", () => {
    open();
    buttonNamed(t("acp.example.summary")).click();
    flushSync();
    expect(box().value).toBe(t("acp.example.summary"));
    expect(document.activeElement).toBe(box());
  });

  it("empty and idle, a button above the examples continues the last conversation, named by its title", async () => {
    const resume = vi.spyOn(acp, "continueLast").mockResolvedValue();
    const last = { title: "Notes", at: 1, supports: "load" as const };
    if (acp.status === null) throw new Error("no status");
    const idle = { ...acp.status, state: "idle" as const, last };
    acp.status = idle;
    open();
    const button = root.querySelector<HTMLButtonElement>("button.continue");
    if (button === null) throw new Error("no continue button");
    expect(button.textContent).toContain(t("acp.continue"));
    expect(button.textContent).toContain("Notes");
    // before the example requests
    const examples = root.querySelector(".examples");
    expect(
      examples !== null &&
        button.compareDocumentPosition(examples) & Node.DOCUMENT_POSITION_FOLLOWING,
    ).toBeTruthy();
    button.click();
    await settle();
    expect(resume).toHaveBeenCalledOnce();
    expect(document.activeElement).toBe(box());
    // no title told: the words alone; a conversation on screen | an agent running: no offer
    acp.status = { ...idle, last: { ...last, title: null } };
    flushSync();
    expect(root.querySelector("button.continue")?.textContent?.trim()).toBe(t("acp.continue"));
    acp.status = { ...idle, state: "ready" };
    flushSync();
    expect(root.querySelector("button.continue")).toBeNull();
    acp.status = idle;
    acp.take({ type: "text", text: "Hello." });
    flushSync();
    expect(root.querySelector("button.continue")).toBeNull();
  });

  it("a failure's own output waits under Details; unfinished tools say Stopped; a new agent draws a line", () => {
    acp.take({ type: "tool", id: "t", title: "Edit a.md", kind: "edit", status: "in_progress" });
    acp.take({ type: "turnEnd", stopReason: "cancelled", changed: 0 });
    acp.take({
      type: "status",
      state: "error",
      error: { code: "exited", stderr: "Error: <b>boom</b>" },
    });
    acp.transcript = [...acp.transcript, { kind: "divider", id: 999, name: "Codex" }];
    acp.status = acp.status === null ? null : { ...acp.status, skippedFiles: 2 };
    open();
    expect(root.querySelector(".tool-status")?.textContent).toBe(t("acp.tool.stopped"));
    const output = root.querySelector(".notice details pre");
    expect(output?.textContent).toBe("Error: <b>boom</b>");
    expect(output?.querySelector("b")).toBeNull();
    expect(root.querySelector(".notice details summary")?.textContent).toBe(t("acp.details"));
    expect(root.querySelector(".divider")?.textContent).toBe(t("acp.switched", { name: "Codex" }));
    expect(root.textContent).toContain(tn("acp.skipped", 2));
  });

  it("the passage chip names the document, lines and the passage's start, and can be removed", () => {
    acp.passage = { path: "docs/a.md", fromLine: 2, toLine: 5, text: "  Open a\nfolder  " };
    open();
    const label = t("acp.passage", { path: "docs/a.md", from: 2, to: 5 });
    expect(root.querySelector(".chip-text")?.textContent).toBe(t("acp.attached", { label }));
    expect(root.querySelector(".chip")?.getAttribute("aria-label")).toBe(
      t("acp.attachedName", { label, start: "Open a folder" }),
    );
    root.querySelector<HTMLButtonElement>(`button[aria-label="${t("acp.dropPassage")}"]`)?.click();
    flushSync();
    expect(acp.passage).toBeNull();
    expect(root.querySelector(".chip")).toBeNull();
  });

  it("each hunk is accepted | rejected alone; a file taken as a whole shows only its file buttons", () => {
    const decide = vi.spyOn(acp, "decide").mockResolvedValue();
    acp.changes = [
      {
        path: "docs/a.md",
        kind: "modified",
        hunks: [
          {
            key: "a3",
            oldStart: 3,
            oldLines: ["old"],
            newLines: ["new"],
            truncated: false,
            contextBefore: null,
            contextAfter: null,
          },
          {
            key: "a9",
            oldStart: 9,
            oldLines: [],
            newLines: ["added"],
            truncated: false,
            contextBefore: null,
            contextAfter: null,
          },
        ],
      },
      {
        path: "docs/new.md",
        kind: "added",
        hunks: [
          {
            key: "n1",
            oldStart: 1,
            oldLines: [],
            newLines: ["# New"],
            truncated: false,
            contextBefore: null,
            contextAfter: null,
          },
        ],
      },
    ];
    open();
    expect(root.textContent).toContain(t("acp.hunkLine", { line: 3 }));
    expect(root.textContent).toContain(t("acp.hunkBefore", { line: 9 }));
    const accepts = [...root.querySelectorAll("button")].filter(
      (b) => b.textContent?.trim() === t("acp.accept"),
    );
    // two hunks of the first file + the second file's own button
    expect(accepts).toHaveLength(3);
    accepts[1]?.click();
    expect(decide).toHaveBeenLastCalledWith("accept", "docs/a.md", ["a9"]);
    root
      .querySelector<HTMLButtonElement>(
        `button[aria-label="${t("acp.rejectFile", { path: "docs/new.md" })}"]`,
      )
      ?.click();
    // the whole file by the keys shown: a list that changed meanwhile is refused
    expect(decide).toHaveBeenLastCalledWith("reject", "docs/new.md", ["n1"]);
    // only added lines: no blank removed line drawn before them
    expect(root.querySelectorAll(".row.del")).toHaveLength(1);
  });

  it("a bar above the message box leads to the changes; Open shows the real file at the line", () => {
    const navigate = vi.spyOn(documents, "navigate").mockReturnValue();
    acp.changes = [
      {
        path: "docs/a.md",
        kind: "modified",
        hunks: [
          {
            key: "a3",
            oldStart: 3,
            oldLines: ["old"],
            newLines: ["new"],
            truncated: false,
            contextBefore: null,
            contextAfter: null,
          },
        ],
      },
      { path: "docs/new.md", kind: "added", hunks: [] },
    ];
    open();
    buttonNamed(t("acp.changesBar", { n: 2 })).click();
    expect(document.activeElement?.id).toBe(root.querySelector(".changes h3")?.id);
    labelled(t("acp.openAt", { path: "docs/a.md", line: 3 })).click();
    expect(navigate).toHaveBeenLastCalledWith("docs/a.md", null, 3);
    labelled(t("acp.openFile", { path: "docs/a.md" })).click();
    expect(navigate).toHaveBeenLastCalledWith("docs/a.md", null, 3);
    // a new file has no real one to open yet
    expect(
      root.querySelector(`[aria-label="${t("acp.openFile", { path: "docs/new.md" })}"]`),
    ).toBeNull();
    // each run is named by its file and place
    expect(root.querySelector(".hunk")?.getAttribute("aria-label")).toBe(
      t("acp.hunkGroup", { path: "docs/a.md", where: t("acp.hunkLine", { line: 3 }) }),
    );
  });

  it("a run shows the real line above and below it as unchanged rows; a whole file shows none", () => {
    acp.changes = [
      {
        path: "docs/a.md",
        kind: "modified",
        hunks: [
          {
            key: "a3",
            oldStart: 3,
            oldLines: ["old"],
            newLines: ["new"],
            truncated: false,
            contextBefore: "above",
            contextAfter: "below",
          },
        ],
      },
      {
        path: "docs/new.md",
        kind: "added",
        hunks: [
          {
            key: "n1",
            oldStart: 1,
            oldLines: [],
            newLines: ["# New"],
            truncated: false,
            contextBefore: "never",
            contextAfter: "never",
          },
        ],
      },
    ];
    open();
    const rows = (hunk: Element | undefined) =>
      [...(hunk?.querySelectorAll(".row") ?? [])].map((row) => [
        row.classList.contains("same") ? "same" : row.classList.contains("add") ? "add" : "del",
        row.querySelector(".text")?.textContent,
      ]);
    const [modified, added] = root.querySelectorAll(".hunk");
    expect(rows(modified)).toEqual([
      ["same", "above"],
      ["del", "old"],
      ["add", "new"],
      ["same", "below"],
    ]);
    expect(rows(added)).toEqual([["add", "# New"]]);
  });

  it("a blank line above | below a run places nothing: no empty row is drawn for it", () => {
    acp.changes = [
      {
        path: "docs/a.md",
        kind: "modified",
        hunks: [
          {
            key: "a7",
            oldStart: 7,
            oldLines: ["| a |"],
            newLines: ["| a |", "| b |"],
            truncated: false,
            contextBefore: "",
            contextAfter: "  ",
          },
        ],
      },
    ];
    open();
    const rows = [...root.querySelectorAll(".hunk .row")].map((row) => [
      row.classList.contains("same") ? "same" : row.classList.contains("add") ? "add" : "del",
      row.querySelector(".text")?.textContent,
    ]);
    expect(rows).toEqual([
      ["same", "| a |"],
      ["add", "| b |"],
    ]);
  });

  it("a long run draws its first rows until Show all; a run the backend cut says so", () => {
    const lines = Array.from({ length: 450 }, (_, i) => `line ${String(i)}`);
    acp.changes = [
      {
        path: "big.md",
        kind: "added",
        hunks: [
          {
            key: "b",
            oldStart: 1,
            oldLines: [],
            newLines: lines,
            truncated: true,
            contextBefore: null,
            contextAfter: null,
          },
        ],
      },
    ];
    open();
    expect(root.querySelectorAll(".row.add")).toHaveLength(200);
    expect(root.textContent).toContain(t("acp.truncated"));
    buttonNamed(t("acp.showAllLines", { n: 450 })).click();
    flushSync();
    expect(root.querySelectorAll(".row.add")).toHaveLength(450);
  });

  it("after a decision the keyboard goes to the next change, else the message box", async () => {
    vi.spyOn(acp, "decide").mockImplementation(async () => {
      acp.changes = [];
    });
    acp.changes = [
      {
        path: "docs/a.md",
        kind: "modified",
        hunks: [
          {
            key: "a3",
            oldStart: 3,
            oldLines: ["old"],
            newLines: ["new"],
            truncated: false,
            contextBefore: null,
            contextAfter: null,
          },
        ],
      },
    ];
    open();
    const accept = labelled(t("acp.acceptFile", { path: "docs/a.md" }));
    accept.focus();
    accept.click();
    await settle();
    expect(document.activeElement).toBe(box());
  });

  it("after a decision on a hunk the keyboard goes to the same answer of the next one", async () => {
    const hunk = (key: string, oldStart: number) => ({
      key,
      oldStart,
      oldLines: ["old"],
      newLines: ["new"],
      truncated: false,
      contextBefore: null,
      contextAfter: null,
    });
    vi.spyOn(acp, "decide").mockImplementation(async (_choice, _path, keys) => {
      acp.changes = acp.changes.map((f) => ({
        ...f,
        hunks: f.hunks.filter((h) => !(keys ?? []).includes(h.key)),
      }));
    });
    acp.changes = [
      {
        path: "docs/a.md",
        kind: "modified",
        hunks: [hunk("a3", 3), hunk("a9", 9), hunk("a20", 20)],
      },
    ];
    open();
    const accepts = () => [...root.querySelectorAll<HTMLButtonElement>(".hunk .btn--primary")];
    const rejects = () => [
      ...root.querySelectorAll<HTMLButtonElement>(".hunk .btn:not(.btn--primary)"),
    ];
    accepts()[0]?.focus();
    accepts()[0]?.click();
    await settle();
    expect(document.activeElement).toBe(accepts()[0]);
    rejects()[0]?.focus();
    rejects()[0]?.click();
    await settle();
    expect(document.activeElement).toBe(rejects()[0]);
  });
});

describe("the agent's settings", () => {
  /** The agent in `state`, offering the mock's settings. */
  function withOptions(state: "ready" | "starting"): void {
    if (acp.status !== null) acp.status = { ...acp.status, state, options: [...MOCK_OPTIONS] };
  }

  function control<E extends HTMLElement>(label: string, kind: new () => E): E {
    const found = [...root.querySelectorAll("label")].find(
      (l) => l.textContent?.trim().startsWith(label) === true,
    );
    const inner = found?.querySelector("select, input");
    if (!(inner instanceof kind)) throw new Error(`no control labelled ${label}`);
    return inner;
  }

  it("show under the agent's name as labelled controls in one group; a change goes to the agent", () => {
    const set = vi.spyOn(acp, "setOption").mockResolvedValue(true);
    withOptions("ready");
    open();
    const group = root.querySelector('header [role="group"]');
    expect(group?.getAttribute("aria-label")).toBe(t("acp.options"));
    const model = control(t("acp.option.model"), HTMLSelectElement);
    expect(model.value).toBe("default");
    expect([...model.options].map((o) => o.textContent)).toEqual([
      "Default (recommended)",
      "Opus 5.5",
      "Sonnet 5",
      "Haiku 5",
    ]);
    // a reasoning level is named as the agent names it
    expect(control("Effort", HTMLSelectElement).value).toBe("default");
    const fast = control("Fast mode", HTMLInputElement);
    expect(fast.getAttribute("role")).toBe("switch");
    expect(fast.checked).toBe(false);
    model.focus();
    model.value = "haiku";
    model.dispatchEvent(new Event("change", { bubbles: true }));
    model.blur();
    fast.click();
    expect(set.mock.calls).toEqual([
      ["model", "haiku"],
      ["fast", true],
    ]);
  });

  it("steps through a closed list reach the agent once: the value the person stops at", () => {
    const set = vi.spyOn(acp, "setOption").mockResolvedValue(true);
    withOptions("ready");
    open();
    const model = control(t("acp.option.model"), HTMLSelectElement);
    const step = (value: string): void => {
      model.value = value;
      model.dispatchEvent(new Event("change", { bubbles: true }));
    };
    vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout"] });
    try {
      model.focus();
      // arrow keys on a closed list (Windows): one change per step
      for (const value of ["opus", "sonnet", "haiku"]) {
        step(value);
        vi.advanceTimersByTime(OPTION_SETTLE_MS - 1);
      }
      expect(set).not.toHaveBeenCalled();
      vi.advanceTimersByTime(1);
      expect(set.mock.calls).toEqual([["model", "haiku"]]);
      // the keyboard leaves the list: what it shows goes at once, once
      step("opus");
      model.blur();
      vi.advanceTimersByTime(OPTION_SETTLE_MS * 2);
      expect(set.mock.calls).toEqual([
        ["model", "haiku"],
        ["model", "opus"],
      ]);
      // the panel goes while a step waits: the step still reaches the agent
      model.focus();
      step("sonnet");
      if (component) void unmount(component);
      component = null;
      flushSync();
      expect(set.mock.calls.at(-1)).toEqual(["model", "sonnet"]);
    } finally {
      vi.useRealTimers();
    }
  });

  it("news of the values while a step waits never sends the step early", () => {
    const set = vi.spyOn(acp, "setOption").mockResolvedValue(true);
    withOptions("ready");
    open();
    const model = control(t("acp.option.model"), HTMLSelectElement);
    vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout"] });
    try {
      model.focus();
      model.value = "opus";
      model.dispatchEvent(new Event("change", { bubbles: true }));
      // the agent tells its values again (an echo, fresh objects) mid-way through the steps
      acp.take({ type: "options", options: structuredClone([...acp.options]) });
      flushSync();
      expect(set).not.toHaveBeenCalled();
      vi.advanceTimersByTime(OPTION_SETTLE_MS);
      expect(set.mock.calls).toEqual([["model", "opus"]]);
    } finally {
      vi.useRealTimers();
    }
  });

  it("wait while the agent starts, and never show over the picker", () => {
    withOptions("starting");
    open();
    expect(control(t("acp.option.model"), HTMLSelectElement).disabled).toBe(true);
    expect(control("Fast mode", HTMLInputElement).disabled).toBe(true);
    acp.openPicker();
    flushSync();
    expect(root.querySelector('header [role="group"]')).toBeNull();
  });

  it("a refused change shows the value that holds again; news of new values keeps the keyboard where it is", async () => {
    vi.spyOn(acp, "setOption").mockResolvedValue(false);
    withOptions("ready");
    open();
    const model = control(t("acp.option.model"), HTMLSelectElement);
    model.focus();
    model.value = "opus";
    model.dispatchEvent(new Event("change", { bubbles: true }));
    model.blur();
    await settle();
    expect(model.value).toBe("default");
    model.focus();
    // the agent tells a new model: the same control shows it, still with the keyboard
    acp.take({
      type: "options",
      options: MOCK_OPTIONS.map((o) =>
        o.kind.type === "select" && o.id === "model"
          ? { ...o, kind: { ...o.kind, current: "sonnet" } }
          : o,
      ),
    });
    flushSync();
    expect(control(t("acp.option.model"), HTMLSelectElement)).toBe(model);
    expect(model.value).toBe("sonnet");
    expect(document.activeElement).toBe(model);
    // a refusal of the agent itself tells the values again as they are (fresh from the backend): the control follows
    model.value = "haiku";
    acp.take({ type: "options", options: structuredClone([...acp.options]) });
    flushSync();
    expect(model.value).toBe("sonnet");
  });
});

describe("the choice of the agent", () => {
  it("none chosen: the picker stands in for the conversation, every agent with how it starts", () => {
    firstRun();
    open();
    expect(root.textContent).toContain(t("acp.pick.title"));
    expect(root.querySelector("textarea")).toBeNull();
    expect(root.textContent).not.toContain(t("acp.empty"));
    const names = [...root.querySelectorAll(".agents .name")].map((n) => n.textContent);
    // the popular ones in their own order, then all the others A-Z
    expect(names).toEqual([
      "Claude Agent",
      "Codex",
      "Gemini CLI",
      "goose",
      "Amp",
      "Cline",
      "Deep Agents",
      "fast-agent",
    ]);
    const groups = [...root.querySelectorAll(".group")].map((g) => g.textContent?.trim());
    expect(groups).toEqual([t("acp.pick.popular"), t("acp.pick.all")]);
    expect(root.textContent).toContain(t("acp.pick.signInHint"));
    const chips = [...root.querySelectorAll(".agents .how")].map((c) => c.textContent);
    expect(chips).toEqual([
      t("acp.how.npx"),
      t("acp.how.npx"),
      t("acp.how.npx"),
      t("acp.how.native"),
      t("acp.needs.install"),
      t("acp.needs.node"),
      t("acp.needs.uv"),
      t("acp.how.uvx"),
    ]);
    // the own command comes before the long list, never below it
    const list = root.querySelector(".agents");
    if (list === null) throw new Error("no list");
    expect(
      buttonNamed(t("acp.pick.other")).compareDocumentPosition(list) &
        Node.DOCUMENT_POSITION_FOLLOWING,
    ).toBeTruthy();
    expect(acp.loadAgents).toHaveBeenCalled();
  });

  it("popular agents are described in PerMark's words, others in the list's; the intro is one sentence, the rest behind a disclosure", () => {
    firstRun();
    open();
    const about = (name: string): string | null | undefined =>
      [...root.querySelectorAll(".agent")]
        .find((a) => a.querySelector(".name")?.textContent === name)
        ?.querySelector(".description")?.textContent;
    expect(about("Claude Agent")).toBe(t("acp.about.claude-acp"));
    expect(about("Codex")).toBe(t("acp.about.codex-acp"));
    expect(about("goose")).toBe(t("acp.about.goose"));
    expect(root.textContent).not.toContain("ACP wrapper for Anthropic");
    expect(about("Cline")).toBe(MOCK_AGENTS.find((a) => a.id === "cline")?.description);
    expect(root.textContent).toContain(t("acp.pick.introShort"));
    const more = root.querySelector(".picker details.intro");
    expect(more?.querySelector("summary")?.textContent).toBe(t("acp.pick.more"));
    expect(more?.hasAttribute("open")).toBe(false);
    expect(more?.textContent).toContain(t("acp.pick.intro"));
    expect(more?.textContent).toContain(t("acp.pick.signInHint"));
  });

  it("an own command shows as Own command with its command line, in the list and the header", () => {
    const typed = '"C:\\a b\\node.exe" agent.mjs --acp';
    acp.status = {
      state: "needsLogin",
      agent: typed,
      agentName: null,
      chosen: true,
      error: null,
      methods: [...MOCK_LOGIN_METHODS],
      skippedFiles: 0,
      last: null,
      options: [],
    };
    open();
    expect(root.querySelector(".agent")?.textContent).toContain(t("acp.ownCommand"));
    expect(root.textContent).not.toContain("node.exe ·");
    // an own command has no name of its own: the sign-in speaks of "the agent"
    expect(root.textContent).toContain(t("acp.login.titleAgent"));
    labelled(t("acp.changeAgent")).click();
    acp.catalog = {
      status: "ready",
      agents: [
        ...MOCK_AGENTS.map((a) => ({ ...a, chosen: false })),
        {
          id: "custom",
          name: "",
          description: "",
          version: "",
          how: "custom",
          needs: null,
          command: typed,
          website: null,
          chosen: true,
        },
      ],
    };
    flushSync();
    const first = root.querySelector(".agents .agent");
    expect(first?.querySelector(".name")?.textContent).toBe(t("acp.ownCommand"));
    expect(first?.querySelector(".command")?.textContent).toBe(typed);
    // its name says it is the person's command: no chip repeats it (the name keeps its room)
    expect(first?.querySelector(".how")).toBeNull();
  });

  it("Choose for an agent that can start; one that cannot says why and where to get what it needs", () => {
    const choose = vi.spyOn(acp, "choose").mockResolvedValue(true);
    const website = vi.spyOn(acp, "openWebsite").mockReturnValue();
    firstRun();
    open();
    labelled(t("acp.pick.chooseNamed", { name: "Codex" })).click();
    expect(choose).toHaveBeenCalledWith({ agent: "codex-acp" });
    // no Choose that does nothing: the reason and the next step instead
    expect(
      root.querySelector(`[aria-label="${t("acp.pick.chooseNamed", { name: "Cline" })}"]`),
    ).toBeNull();
    const cline = [...root.querySelectorAll(".agent")].find((a) =>
      a.textContent?.includes("Cline"),
    );
    expect(cline?.textContent).toContain(t("acp.why.node"));
    buttonNamed(t("acp.pick.installNode")).click();
    expect(website).toHaveBeenLastCalledWith("https://nodejs.org/en/download");
    buttonNamed(t("acp.pick.installUv")).click();
    expect(website).toHaveBeenLastCalledWith(
      "https://docs.astral.sh/uv/getting-started/installation/",
    );
    labelled(t("acp.pick.howToInstallNamed", { name: "Amp" })).click();
    expect(website).toHaveBeenLastCalledWith("https://github.com/tao12345666333/amp-acp");
    // ready agents need no install
    expect(
      root.querySelector(`[aria-label="${t("acp.pick.howToInstallNamed", { name: "Codex" })}"]`),
    ).toBeNull();
    // the command each runs waits under Details
    const codex = [...root.querySelectorAll(".agent")].find((a) =>
      a.textContent?.includes("Codex"),
    );
    expect(codex?.querySelector("details summary")?.textContent).toBe(t("acp.details"));
    expect(codex?.querySelector("details .command")?.textContent).toContain("codex-acp");
  });

  it("the agent in use says so; Escape and the top row's Cancel go back to the conversation", async () => {
    open();
    labelled(t("acp.changeAgent")).click();
    acp.catalog = {
      status: "ready",
      agents: MOCK_AGENTS.map((a) => ({ ...a, chosen: a.id === "claude-acp" })),
    };
    flushSync();
    const inUse = labelled(t("acp.pick.inUseNamed", { name: "Claude Agent" }));
    expect(inUse.textContent?.trim()).toBe(t("acp.pick.inUse"));
    expect(inUse.disabled).toBe(true);
    // Cancel sits in the row above the list
    expect(root.querySelector(".title-row")?.textContent).toContain(t("common.cancel"));
    root
      .querySelector('input[type="search"]')
      ?.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
    await settle();
    expect(acp.showsPicker).toBe(false);
    expect(document.activeElement).toBe(box());
  });

  it("inside a Snap one line says why nothing can start, instead of the list", () => {
    firstRun();
    acp.status = acp.status === null ? null : { ...acp.status, needs: "snap" };
    open();
    expect(root.textContent).toContain(t("acp.snap"));
    expect(root.querySelector(".agents")).toBeNull();
    expect(root.querySelector('input[type="search"]')).toBeNull();
  });

  it("the search keeps the agents with every word typed", () => {
    firstRun();
    open();
    const search = root.querySelector<HTMLInputElement>('input[type="search"]');
    if (search === null) throw new Error("no search field");
    search.value = "coding agent";
    search.dispatchEvent(new Event("input", { bubbles: true }));
    flushSync();
    const names = [...root.querySelectorAll(".agents .name")].map((n) => n.textContent);
    // a popular agent by PerMark's words for it too
    expect(names).toEqual(["Codex", "Amp", "Cline"]);
    search.value = "zzz";
    search.dispatchEvent(new Event("input", { bubbles: true }));
    flushSync();
    expect(root.textContent).toContain(t("acp.pick.none"));
  });

  it("Other command takes the person's own command, with the line that PerMark starts it", async () => {
    const choose = vi.spyOn(acp, "choose").mockResolvedValue(true);
    firstRun();
    open();
    buttonNamed(t("acp.pick.other")).click();
    await Promise.resolve();
    flushSync();
    expect(root.textContent).toContain(t("acp.pick.commandWarning"));
    const field = root.querySelector<HTMLInputElement>("form input");
    if (field === null) throw new Error("no command field");
    expect(buttonNamed(t("acp.pick.useCommand")).disabled).toBe(true);
    field.value = "my-agent --acp";
    field.dispatchEvent(new Event("input", { bubbles: true }));
    flushSync();
    buttonNamed(t("acp.pick.useCommand")).click();
    expect(choose).toHaveBeenCalledWith({ command: "my-agent --acp" });
  });

  it("after a choice the keyboard moves on to the message box, never to the page", async () => {
    vi.spyOn(acp, "choose").mockImplementation(async () => {
      acp.status = {
        state: "idle",
        agent: "npx -y @agentclientprotocol/codex-acp@2.7.0",
        agentName: "Codex",
        chosen: true,
        error: null,
        methods: [],
        skippedFiles: 0,
        last: null,
        options: [],
      };
      acp.picking = false;
      return true;
    });
    firstRun();
    open();
    const choose = labelled(t("acp.pick.chooseNamed", { name: "Codex" }));
    choose.focus();
    choose.click();
    await settle();
    expect(document.activeElement).toBe(box());
  });

  it("after an own command is taken | Cancel the keyboard moves on to the message box", async () => {
    vi.spyOn(acp, "choose").mockImplementation(async () => {
      acp.picking = false;
      return true;
    });
    open();
    labelled(t("acp.changeAgent")).click();
    flushSync();
    buttonNamed(t("acp.pick.other")).click();
    await settle();
    const field = root.querySelector<HTMLInputElement>("form input");
    if (field === null) throw new Error("no command field");
    field.value = '"C:\\a b\\node.exe" agent.mjs';
    field.dispatchEvent(new Event("input", { bubbles: true }));
    flushSync();
    buttonNamed(t("acp.pick.useCommand")).click();
    await settle();
    expect(document.activeElement).toBe(box());

    labelled(t("acp.changeAgent")).click();
    flushSync();
    buttonNamed(t("common.cancel")).click();
    await settle();
    expect(document.activeElement).toBe(box());
  });

  it("the header's Change opens the picker, waits while the agent works, and Cancel goes back", () => {
    open();
    const change = labelled(t("acp.changeAgent"));
    acp.take({ type: "status", state: "working", error: null });
    flushSync();
    expect(change.disabled).toBe(true);
    acp.take({ type: "status", state: "ready", error: null });
    flushSync();
    expect(change.disabled).toBe(false);
    change.click();
    flushSync();
    expect(root.textContent).toContain(t("acp.pick.title"));
    expect(root.querySelector("textarea")).toBeNull();
    buttonNamed(t("common.cancel")).click();
    flushSync();
    expect(root.textContent).not.toContain(t("acp.pick.title"));
    expect(root.querySelector("textarea")).not.toBeNull();
  });
});

describe("the sign-in card", () => {
  it("offers each way as it works, then Try again", async () => {
    const login = vi.spyOn(acp, "login").mockResolvedValue();
    const copy = vi.spyOn(acp, "copy").mockResolvedValue(true);
    acp.take({
      type: "status",
      state: "needsLogin",
      error: null,
      methods: [...MOCK_LOGIN_METHODS],
    });
    open();
    expect(root.textContent).toContain(t("acp.state.needsLogin"));
    expect(root.textContent).toContain(t("acp.login.title", { name: "Claude Agent" }));
    const [agentWay, terminal, env] = MOCK_LOGIN_METHODS;
    if (agentWay === undefined || terminal?.hint == null || env?.hint == null)
      throw new Error("mock methods changed");
    buttonNamed(agentWay.name).click();
    expect(login).toHaveBeenLastCalledWith(agentWay);
    // the card goes once signed in: the keyboard moves on to the message box at once
    await settle();
    expect(document.activeElement).toBe(box());
    // a terminal way: its command to copy, never run by the app; folded under Advanced beside the agent's own way
    expect(root.querySelector(".login .command")?.textContent).toBe(terminal.hint);
    expect(root.querySelector(".login details summary")?.textContent).toBe(
      t("acp.login.terminalWay"),
    );
    expect(root.querySelector(".login .command")?.getAttribute("tabindex")).toBe("0");
    labelledTitle(t("acp.login.copyCommand")).click();
    expect(copy).toHaveBeenCalledWith(terminal.hint);
    expect(root.textContent).toContain(t("acp.login.env", { names: env.hint }));
    const retry = buttonNamed(t("acp.retry"));
    retry.focus();
    retry.click();
    expect(login).toHaveBeenLastCalledWith(null);
    await settle();
    expect(document.activeElement).toBe(box());
  });
});

describe("the sign-in card's copy", () => {
  it("a copy the system refused is said, so the person copies by hand", async () => {
    vi.spyOn(acp, "copy").mockResolvedValue(false);
    acp.take({
      type: "status",
      state: "needsLogin",
      error: null,
      methods: [...MOCK_LOGIN_METHODS],
    });
    open();
    labelledTitle(t("acp.login.copyCommand")).click();
    await settle();
    expect(root.textContent).toContain(t("acp.login.copyFailed"));
  });
});

function labelledTitle(title: string): HTMLButtonElement {
  const found = root.querySelector(`button[title="${title}"]`);
  if (!(found instanceof HTMLButtonElement)) throw new Error(`no button titled ${title}`);
  return found;
}

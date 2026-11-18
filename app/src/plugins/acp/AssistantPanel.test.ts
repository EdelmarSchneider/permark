// @vitest-environment jsdom
// The Assistant panel: the agent's text as plain text (markup never becomes elements), question cards that answer
// with the agent's options, Enter | Shift+Enter | an IME's Enter in the message box, Send while the agent works, the
// passage chip, and hunks accepted | rejected one by one (files taken only as a whole show no hunk buttons); the
// picker (no agent chosen | Change): search, a chip per state, How to install, Choose only for an agent that can
// start, an own command; the sign-in card (the agent's own way, a command to copy, variables, Try again).
import "../../core/testing/media";
import { flushSync, mount, unmount } from "svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { MOCK_AGENTS, MOCK_LOGIN_METHODS } from "../../api/mockAcp";
import { t } from "../../core/i18n";
import AssistantPanel from "./AssistantPanel.svelte";
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
  };
  acp.catalog = { status: "ready", agents: MOCK_AGENTS.map((a) => ({ ...a, chosen: false })) };
}

beforeEach(() => {
  vi.spyOn(acp, "refreshChanges").mockResolvedValue();
  vi.spyOn(acp, "loadAgents").mockResolvedValue();
  acp.clear();
  acp.status = {
    state: "ready",
    agent: "npx -y @agentclientprotocol/claude-agent-acp@0.108.0",
    agentName: "Claude Agent",
    chosen: true,
    error: null,
    methods: [],
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

  it("shows the agent's text as plain text", () => {
    acp.take({ type: "text", text: "<b>bold</b> <img src=x onerror=alert(1)>" });
    open();
    const reply = root.querySelector(".agent-text .text");
    expect(reply?.textContent).toBe("<b>bold</b> <img src=x onerror=alert(1)>");
    expect(reply?.querySelector("b, img")).toBeNull();
  });

  it("a question answers with the option the person picks, then says what was answered", () => {
    const answer = vi.spyOn(acp, "answer").mockResolvedValue();
    acp.take({
      type: "permission",
      requestId: "7",
      title: "Run tests",
      kind: "execute",
      detail: "npm test",
      options: [
        { id: "yes", name: "Allow", kind: "allow_once" },
        { id: "no", name: "Reject", kind: "reject_once" },
      ],
    });
    open();
    expect(root.textContent).toContain("npm test");
    buttonNamed("Allow").click();
    expect(answer).toHaveBeenCalledWith("7", { id: "yes", name: "Allow", kind: "allow_once" });
    acp.take({ type: "permissionDone", requestId: "7" });
    flushSync();
    expect(root.textContent).toContain(t("acp.questionClosed"));
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

  it("Send waits while the agent works; Stop shows then", () => {
    const cancel = vi.spyOn(acp, "cancel").mockResolvedValue();
    open();
    acp.draft = "Hello";
    flushSync();
    expect(buttonNamed(t("acp.send")).disabled).toBe(false);
    acp.take({ type: "status", state: "working", error: null });
    flushSync();
    expect(buttonNamed(t("acp.send")).disabled).toBe(true);
    root.querySelector<HTMLButtonElement>(`button[aria-label="${t("acp.stop")}"]`)?.click();
    expect(cancel).toHaveBeenCalled();
  });

  it("the passage chip names the document and lines and can be removed", () => {
    acp.passage = { path: "docs/a.md", fromLine: 2, toLine: 5, text: "x" };
    open();
    expect(root.textContent).toContain(t("acp.passage", { path: "docs/a.md", from: 2, to: 5 }));
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
          { id: 0, oldStart: 3, oldLines: ["old"], newLines: ["new"] },
          { id: 1, oldStart: 9, oldLines: [], newLines: ["added"] },
        ],
      },
      {
        path: "docs/new.md",
        kind: "added",
        hunks: [{ id: 0, oldStart: 1, oldLines: [], newLines: ["# New"] }],
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
    expect(decide).toHaveBeenLastCalledWith("accept", "docs/a.md", [1]);
    root
      .querySelector<HTMLButtonElement>(
        `button[aria-label="${t("acp.rejectFile", { path: "docs/new.md" })}"]`,
      )
      ?.click();
    expect(decide).toHaveBeenLastCalledWith("reject", "docs/new.md", null);
    // only added lines: no blank removed line drawn before them
    expect(root.querySelectorAll(".row.del")).toHaveLength(1);
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
    // those that can start first
    expect(names).toEqual([
      "Claude Agent",
      "Codex",
      "Gemini CLI",
      "goose",
      "fast-agent",
      "Amp",
      "Cline",
      "Deep Agents",
    ]);
    const chips = [...root.querySelectorAll(".agents .how")].map((c) => c.textContent);
    expect(chips).toEqual([
      t("acp.how.npx"),
      t("acp.how.npx"),
      t("acp.how.npx"),
      t("acp.how.native"),
      t("acp.how.uvx"),
      t("acp.needs.install"),
      t("acp.needs.node"),
      t("acp.needs.uv"),
    ]);
    expect(acp.loadAgents).toHaveBeenCalled();
  });

  it("Choose works for an agent that can start; How to install opens the website of one that cannot", () => {
    const choose = vi.spyOn(acp, "choose").mockResolvedValue(true);
    const website = vi.spyOn(acp, "openWebsite").mockReturnValue();
    firstRun();
    open();
    labelled(t("acp.pick.chooseNamed", { name: "Codex" })).click();
    expect(choose).toHaveBeenCalledWith({ agent: "codex-acp" });
    expect(labelled(t("acp.pick.chooseNamed", { name: "Cline" })).disabled).toBe(true);
    labelled(t("acp.pick.howToInstallNamed", { name: "Cline" })).click();
    expect(website).toHaveBeenCalledWith("https://cline.bot/cli");
    // ready agents need no install; one without a website has no button for it
    expect(
      root.querySelector(`[aria-label="${t("acp.pick.howToInstallNamed", { name: "Codex" })}"]`),
    ).toBeNull();
    expect(
      root.querySelector(
        `[aria-label="${t("acp.pick.howToInstallNamed", { name: "Deep Agents" })}"]`,
      ),
    ).toBeNull();
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
    expect(names).toEqual(["Amp", "Cline"]);
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
  it("offers each way as it works, then Try again", () => {
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
    // a terminal way: its command to copy, never run by the app
    expect(root.querySelector(".login .command")?.textContent).toBe(terminal.hint);
    labelledTitle(t("acp.login.copyCommand")).click();
    expect(copy).toHaveBeenCalledWith(terminal.hint);
    expect(root.textContent).toContain(t("acp.login.env", { names: env.hint }));
    buttonNamed(t("acp.retry")).click();
    expect(login).toHaveBeenLastCalledWith(null);
  });
});

function labelledTitle(title: string): HTMLButtonElement {
  const found = root.querySelector(`button[title="${title}"]`);
  if (!(found instanceof HTMLButtonElement)) throw new Error(`no button titled ${title}`);
  return found;
}

// @vitest-environment jsdom
// The Assistant panel: the agent's text as plain text (markup never becomes elements), question cards that answer
// with the agent's options, Enter | Shift+Enter | an IME's Enter in the message box, Send while the agent works, the
// passage chip, and hunks accepted | rejected one by one (files taken only as a whole show no hunk buttons).
import "../../core/testing/media";
import { flushSync, mount, unmount } from "svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
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

beforeEach(() => {
  vi.spyOn(acp, "refreshChanges").mockResolvedValue();
  acp.clear();
  acp.status = {
    state: "ready",
    agent: "npx -y @agentclientprotocol/claude-agent-acp@0.108.0",
    error: null,
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
    expect(root.textContent).toContain("claude-agent-acp");
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

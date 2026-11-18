// What the assistant plugin answers in the mock backend: the agent's status, a timed stand-in reply to a message
// (status, thinking, text in pieces, tool lines, one question that waits for the person's answer, then the turn's end
// with one changed file) and the work copy's changes, where accept | reject drop hunks. Nothing is written: the real
// copy, agent and writes are the plugin's (tested there); the mock only feeds the window's flow.

import type { AcpState } from "./generated/AcpState";
import type { AcpUpdate } from "./generated/AcpUpdate";
import type { AnswerArgs } from "./generated/AnswerArgs";
import type { FileChange } from "./generated/FileChange";
import type { PermissionChoice } from "./generated/PermissionChoice";
import type { PromptArgs } from "./generated/PromptArgs";
import { MockPluginError } from "./mockOrganizing";
import type { PluginEventName, PluginEvents, PluginMethods } from "./types";

type AcpMethods = Pick<
  PluginMethods,
  | "acp.status"
  | "acp.prompt"
  | "acp.cancel"
  | "acp.reset"
  | "acp.answer"
  | "acp.changes"
  | "acp.accept"
  | "acp.reject"
>;

/** The agent command the mock reports (the plugin's default). */
export const MOCK_ACP_COMMAND = "npx -y @agentclientprotocol/claude-agent-acp@0.108.0";

export type AcpProbe = {
  /** Calls made, by method name. */
  calls: string[];
  /** What each message sent, in order. */
  prompts: PromptArgs[];
  /** The answers given to the agent's questions, in order. */
  answers: AnswerArgs[];
  /** Time between two news of a reply. Read when a turn starts | goes on. */
  stepMs: number;
  /** The refusal the next accept | reject gives (an unsaved document, a file changed since the copy). */
  fail: MockPluginError | null;
  /** The work copy's changes now (set by the end of the first turn). */
  changes: FileChange[];
  /** Stops the reply timer (tests). */
  stopAll(): void;
};

const GUIDE = "docs/guide.md";

/** The two changes the stand-in agent makes in the guide (lines as in the mock project's file). */
function guideChanges(): FileChange[] {
  return [
    {
      path: GUIDE,
      kind: "modified",
      hunks: [
        {
          id: 0,
          oldStart: 7,
          oldLines: [
            "Open a folder, pick a document in the tree, read it. Links between documents work like a wiki: use Alt+Left to go back.",
          ],
          newLines: [
            "Open a folder, then pick a document in the tree to read it.",
            "Links between documents work like a wiki: press Alt+Left to go back.",
          ],
        },
        {
          id: 1,
          oldStart: 17,
          oldLines: ["| Ctrl+= / Ctrl+- | Zoom         |"],
          newLines: ["| Ctrl+= / Ctrl+- | Zoom         |", "| Ctrl+E       | Edit            |"],
        },
      ],
    },
  ];
}

const OPTIONS: PermissionChoice[] = [
  { id: "allow-once", name: "Allow", kind: "allow_once" },
  { id: "allow-always", name: "Always allow", kind: "allow_always" },
  { id: "reject", name: "Reject", kind: "reject_once" },
];

/** One news of a reply, or the question the reply stops at until it is answered. */
type Step =
  | { kind: "news"; update: AcpUpdate }
  | { kind: "ask"; update: Extract<AcpUpdate, { type: "permission" }> }
  | { kind: "end" };

export function createMockAcp(
  emit: <E extends PluginEventName>(event: E, data: PluginEvents[E]) => void,
): {
  handlers: {
    [M in keyof AcpMethods]: (args: AcpMethods[M]["args"]) => AcpMethods[M]["result"];
  };
  probe: AcpProbe;
} {
  let state: AcpState = "idle";
  let started = false;
  let turn = 0;
  let next: Step[] = [];
  /** The question the reply waits at (`null`: none). */
  let waiting: Extract<AcpUpdate, { type: "permission" }> | null = null;
  let timer: ReturnType<typeof setInterval> | null = null;
  const stopTimer = (): void => {
    if (timer !== null) clearInterval(timer);
    timer = null;
  };
  const probe: AcpProbe = {
    calls: [],
    prompts: [],
    answers: [],
    stepMs: 250,
    fail: null,
    changes: [],
    stopAll: stopTimer,
  };
  const news = (update: AcpUpdate): void => emit("acp:update", update);
  const become = (to: AcpState): void => {
    state = to;
    news({ type: "status", state: to, error: null });
  };
  const run = (): void => {
    stopTimer();
    timer = setInterval(step, probe.stepMs);
  };

  function step(): void {
    const now = next.shift();
    if (now === undefined) {
      stopTimer();
      return;
    }
    switch (now.kind) {
      case "news":
        if (now.update.type === "status") become(now.update.state);
        else news(now.update);
        return;
      case "ask":
        stopTimer();
        waiting = now.update;
        news(now.update);
        return;
      case "end":
        stopTimer();
        if (probe.changes.length === 0) probe.changes = guideChanges();
        news({ type: "turnEnd", stopReason: "end_turn", changed: probe.changes.length });
        become("ready");
        return;
      default:
        now satisfies never;
    }
  }

  /** Closes the question still waiting (the turn ended without its answer). */
  function dropQuestion(): void {
    if (waiting === null) return;
    news({ type: "permissionDone", requestId: waiting.requestId });
    waiting = null;
  }

  function script(args: PromptArgs): Step[] {
    turn += 1;
    const id = (n: number): string => `t${String(turn)}-${String(n)}`;
    const tool = (n: number, title: string, kind: string, status: string): Step => ({
      kind: "news",
      update: { type: "tool", id: id(n), title, kind, status },
    });
    const text = (chunk: string): Step => ({ kind: "news", update: { type: "text", text: chunk } });
    const about =
      args.selection === undefined
        ? "the document"
        : `lines ${String(args.selection.fromLine)}-${String(args.selection.toLine)} of ${args.selection.path}`;
    return [
      { kind: "news", update: { type: "thinking" } },
      tool(1, `Read ${GUIDE}`, "read", "pending"),
      tool(1, `Read ${GUIDE}`, "read", "completed"),
      text(`I read ${about}. `),
      text("Two places can be clearer: "),
      text("the first steps, and the list of shortcuts.\n"),
      tool(2, `Edit ${GUIDE}`, "edit", "in_progress"),
      tool(2, `Edit ${GUIDE}`, "edit", "completed"),
      {
        kind: "ask",
        update: {
          type: "permission",
          requestId: `q${String(turn)}`,
          title: "Run the link check",
          kind: "execute",
          detail: "npm run check-links",
          options: OPTIONS,
        },
      },
    ];
  }

  /** What the reply does after the question got `option` (`null`: dismissed). */
  function afterAnswer(option: PermissionChoice | null): Step[] {
    const allowed = option?.kind.startsWith("allow") === true;
    return [
      {
        kind: "news",
        update: {
          type: "tool",
          id: `t${String(turn)}-3`,
          title: "Run the link check",
          kind: "execute",
          status: allowed ? "completed" : "failed",
        },
      },
      {
        kind: "news",
        update: {
          type: "text",
          text: allowed
            ? "All links work. The changes wait for you below."
            : "I did not run the check. The changes wait for you below.",
        },
      },
      { kind: "end" },
    ];
  }

  /** Ends the turn now (Stop, a new conversation): the question still waiting is closed. */
  function stopTurn(): void {
    stopTimer();
    next = [];
    dropQuestion();
  }

  const refuse = (): void => {
    const fail = probe.fail;
    probe.fail = null;
    if (fail !== null) throw fail;
  };

  function decide(path: string, hunks: number[] | null, accepted: boolean): FileChange[] {
    refuse();
    const file = probe.changes.find((c) => c.path === path);
    if (file === undefined) throw new MockPluginError("notFound", path, "noChange");
    if (hunks === null) {
      probe.changes = probe.changes.filter((c) => c !== file);
      return probe.changes;
    }
    if (file.kind === "binary" || file.kind === "conflict")
      throw new MockPluginError("invalidData", path, "wholeFileOnly");
    if (hunks.some((h) => !file.hunks.some((k) => k.id === h)))
      throw new MockPluginError("invalidData", path, "unknownHunk");
    // an accepted hunk moves the real lines after it, as the plugin's fresh list says
    let shift = 0;
    const kept = file.hunks.flatMap((hunk) => {
      if (hunks.includes(hunk.id)) {
        if (accepted) shift += hunk.newLines.length - hunk.oldLines.length;
        return [];
      }
      return [{ ...hunk, oldStart: hunk.oldStart + shift }];
    });
    probe.changes = probe.changes.flatMap((c) =>
      c !== file
        ? [c]
        : kept.length === 0
          ? []
          : [{ ...c, hunks: kept.map((hunk, id) => ({ ...hunk, id })) }],
    );
    return probe.changes;
  }

  return {
    probe,
    handlers: {
      "acp.status": () => {
        probe.calls.push("status");
        return { state, agent: MOCK_ACP_COMMAND, error: null };
      },
      "acp.prompt": (args) => {
        probe.calls.push("prompt");
        if (args.text.trim() === "") throw new MockPluginError("invalidData", null, "emptyPrompt");
        if (state === "starting" || state === "working")
          throw new MockPluginError("conflict", null, "busy");
        probe.prompts.push(structuredClone(args));
        const steps = script(args);
        if (started) become("working");
        else {
          started = true;
          become("starting");
          steps.unshift({
            kind: "news",
            update: { type: "status", state: "working", error: null },
          });
        }
        next = steps;
        run();
        return null;
      },
      "acp.cancel": () => {
        probe.calls.push("cancel");
        if (state !== "starting" && state !== "working") return null;
        stopTurn();
        news({ type: "turnEnd", stopReason: "cancelled", changed: probe.changes.length });
        become("ready");
        return null;
      },
      "acp.reset": () => {
        probe.calls.push("reset");
        stopTurn();
        started = false;
        become("idle");
        return null;
      },
      "acp.answer": (args) => {
        probe.calls.push("answer");
        const asked = waiting;
        if (asked === null || asked.requestId !== args.requestId)
          throw new MockPluginError("notFound", args.requestId, "noQuestion");
        const option =
          args.optionId === null
            ? null
            : (asked.options.find((o) => o.id === args.optionId) ?? null);
        if (args.optionId !== null && option === null)
          throw new MockPluginError("invalidData", args.optionId, "unknownOption");
        probe.answers.push({ ...args });
        dropQuestion();
        next = afterAnswer(option);
        run();
        return null;
      },
      "acp.changes": () => {
        probe.calls.push("changes");
        return probe.changes;
      },
      "acp.accept": ({ path, hunks }) => {
        probe.calls.push("accept");
        return decide(path, hunks, true);
      },
      "acp.reject": ({ path, hunks }) => {
        probe.calls.push("reject");
        return decide(path, hunks, false);
      },
    },
  };
}

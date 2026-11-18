// What the assistant plugin answers in the mock backend: the agents to choose from (one in each state a computer can
// give: installed, started by npx | uvx, missing Node.js | uv | an install) and the person's choice (none at first, as
// on a first run), the agent's status, a timed stand-in reply to a message (status, thinking, text in pieces, tool
// lines, one question that waits for the person's answer, then the turn's end with one changed file) and the work
// copy's changes, where accept | reject drop hunks (named by key) and undo puts the file's last list entry back.
// Gemini CLI asks for a sign-in at its first start: its own method signs in, "Try again" alone stays signed out. The
// plugin's checks of what it is sent are mirrored (a passage alone is a message, line range, project paths, files
// taken only as a whole, links never accepted, relative program paths, inside a Snap nothing starts), so the window's
// words for them are tested against the same refusals. Nothing is written: the real copy, agent, choice file and
// writes are the plugin's (tested there); the mock only feeds the window's flow.

import type { AcpError } from "./generated/AcpError";
import type { AcpState } from "./generated/AcpState";
import type { AcpToolKind } from "./generated/AcpToolKind";
import type { AcpToolStatus } from "./generated/AcpToolStatus";
import type { AcpUpdate } from "./generated/AcpUpdate";
import type { AgentEntry } from "./generated/AgentEntry";
import type { AnswerArgs } from "./generated/AnswerArgs";
import type { ChangeList } from "./generated/ChangeList";
import type { FileChange } from "./generated/FileChange";
import type { LoginArgs } from "./generated/LoginArgs";
import type { LoginMethod } from "./generated/LoginMethod";
import type { PermissionChoice } from "./generated/PermissionChoice";
import type { PromptArgs } from "./generated/PromptArgs";
import { MockPluginError } from "./mockOrganizing";
import type { PluginEventName, PluginEvents, PluginMethods } from "./types";

type AcpMethods = Pick<
  PluginMethods,
  | "acp.status"
  | "acp.agents"
  | "acp.choose"
  | "acp.login"
  | "acp.prompt"
  | "acp.cancel"
  | "acp.reset"
  | "acp.answer"
  | "acp.changes"
  | "acp.accept"
  | "acp.reject"
  | "acp.undo"
>;

/** The person's choice: an agent of the list | an own command. */
export type MockAcpChoice = { agent: string } | { command: string };

/** The agents the mock computer lists, as the plugin words them (`chosen` set from the choice). */
export const MOCK_AGENTS: readonly Omit<AgentEntry, "chosen">[] = [
  {
    id: "claude-acp",
    name: "Claude Agent",
    description: "ACP wrapper for Anthropic's Claude",
    version: "0.108.0",
    how: "npx",
    needs: null,
    command: "npx -y @agentclientprotocol/claude-agent-acp@0.108.0",
    website: "https://github.com/agentclientprotocol/claude-agent-acp",
  },
  {
    id: "codex-acp",
    name: "Codex",
    description: "ACP adapter for OpenAI's coding assistant",
    version: "2.7.0",
    how: "npx",
    needs: null,
    command: "npx -y @agentclientprotocol/codex-acp@2.7.0",
    website: "https://github.com/agentclientprotocol/codex-acp",
  },
  {
    id: "gemini",
    name: "Gemini CLI",
    description: "Google's official CLI for Gemini",
    version: "0.68.0",
    how: "npx",
    needs: null,
    command: "npx -y @google/gemini-cli@0.68.0 --acp",
    website: "https://geminicli.com",
  },
  {
    id: "goose",
    name: "goose",
    description: "A local, extensible, open source AI agent that automates engineering tasks",
    version: "1.59.2",
    how: "native",
    needs: null,
    command: "goose acp",
    website: "https://block.github.io/goose/",
  },
  {
    id: "fast-agent",
    name: "fast-agent",
    description: "Code and build agents with comprehensive multi-provider support",
    version: "0.10.1",
    how: "uvx",
    needs: null,
    command: "uvx fast-agent-acp==0.10.1 -x",
    website: "https://fast-agent.ai",
  },
  {
    id: "amp-acp",
    name: "Amp",
    description: "ACP wrapper for Amp - the frontier coding agent",
    version: "0.13.0",
    how: "missing",
    needs: "install",
    command: "amp-acp.exe",
    website: "https://github.com/tao12345666333/amp-acp",
  },
  {
    id: "cline",
    name: "Cline",
    description:
      "Autonomous coding agent CLI - capable of creating/editing files, running commands, using the browser, and more",
    version: "3.0.74",
    how: "missing",
    needs: "node",
    command: "npx -y cline@3.0.74 --acp",
    website: "https://cline.bot/cli",
  },
  {
    id: "deepagents",
    name: "Deep Agents",
    description: "Agent harness with planning, file system and sub-agents",
    version: "0.4.2",
    how: "missing",
    needs: "uv",
    command: "uvx deepagents-acp==0.4.2",
    website: null,
  },
];

/** How Gemini CLI offers to sign in: its own way, a terminal command, a key in a variable. */
export const MOCK_LOGIN_METHODS: readonly LoginMethod[] = [
  {
    id: "oauth-personal",
    name: "Log in with Google",
    description: null,
    kind: "agent",
    hint: null,
  },
  {
    id: "terminal-login",
    name: "Sign in from a terminal",
    description: "Opens the sign-in in your browser.",
    kind: "terminal",
    hint: "npx -y @google/gemini-cli@0.68.0 --acp --login",
  },
  {
    id: "gemini-api-key",
    name: "Use Gemini API key",
    description: "Requires setting the GEMINI_API_KEY environment variable",
    kind: "env",
    hint: "GEMINI_API_KEY",
  },
];

/** What a "Try again" without a sign-in answers (the window words the code). */
export const MOCK_NOT_SIGNED_IN: AcpError = { code: "stillNeedsLogin" };

export type AcpProbe = {
  /** Calls made, by method name. */
  calls: string[];
  /** The person's choice (`null`: none, as on a first run). */
  choice: MockAcpChoice | null;
  /** Agents that ask for a sign-in at their next start, until one of their own methods signs in. */
  signedOut: Set<string>;
  /** The sign-ins asked for, in order. */
  logins: LoginArgs[];
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
  /** Files the copy left out for their size (`AcpStatus.skippedFiles`). */
  skipped: number;
  /** PerMark runs inside a Snap: nothing can start, no choice is kept. */
  snap: boolean;
  /** Project paths agents may not see: a passage of one is refused, such an open document is not named. */
  hidden: Set<string>;
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
          key: "guide-7",
          oldStart: 7,
          oldLines: [
            "Open a folder, pick a document in the tree, read it. Links between documents work like a wiki: use Alt+Left to go back.",
          ],
          newLines: [
            "Open a folder, then pick a document in the tree to read it.",
            "Links between documents work like a wiki: press Alt+Left to go back.",
          ],
          truncated: false,
        },
        {
          key: "guide-17",
          oldStart: 17,
          oldLines: ["| Ctrl+= / Ctrl+- | Zoom         |"],
          newLines: ["| Ctrl+= / Ctrl+- | Zoom         |", "| Ctrl+E       | Edit            |"],
          truncated: false,
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
  /** The agent wants a sign-in before the reply goes on (`then`: its first steps after it; `why`: a failed one). */
  | { kind: "login"; then: Step[]; why: AcpError | null }
  | { kind: "end" };

/** A project path of a file, as the plugin parses it: relative, `/` separated, no `.` | `..` step, not the folder. */
function projectFile(path: string): void {
  const steps = path.split(/[\\/]/);
  if (
    path === "" ||
    /^([A-Za-z]:|[\\/])/.test(path) ||
    steps.some((step) => step === "" || step === "." || step === "..")
  )
    throw new MockPluginError("invalidPath", path, null);
}

/** Refuses a prompt the plugin would refuse before anything starts. */
function check(args: PromptArgs): void {
  if (args.text.trim() === "" && args.selection === undefined)
    throw new MockPluginError("invalidData", null, "emptyPrompt");
  const selection = args.selection;
  if (selection !== undefined) {
    projectFile(selection.path);
    if (selection.fromLine === 0 || selection.toLine < selection.fromLine)
      throw new MockPluginError("invalidData", selection.path, "lineRange");
  }
  if (args.document !== undefined) projectFile(args.document);
}

/** The command's program (its first word; double quotes group) is a path with a folder that is not absolute. */
function relativeProgram(command: string): boolean {
  const program = command.startsWith('"')
    ? command.slice(1, command.indexOf('"', 1))
    : (command.split(/\s+/)[0] ?? "");
  const absolute = /^([A-Za-z]:[\\/]|[\\/])/.test(program);
  return /[\\/]/.test(program) && !absolute;
}

export function createMockAcp(
  emit: <E extends PluginEventName>(event: E, data: PluginEvents[E]) => void,
): {
  handlers: {
    [M in keyof AcpMethods]: (args: AcpMethods[M]["args"]) => AcpMethods[M]["result"];
  };
  probe: AcpProbe;
} {
  let state: AcpState = "idle";
  let error: AcpError | null = null;
  let started = false;
  /** The first steps of the reply that waits for a sign-in (`null`: none waits). */
  let afterLogin: Step[] | null = null;
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
    choice: null,
    signedOut: new Set(["gemini"]),
    logins: [],
    prompts: [],
    answers: [],
    stepMs: 250,
    fail: null,
    changes: [],
    skipped: 0,
    snap: false,
    hidden: new Set(),
    stopAll: stopTimer,
  };
  const news = (update: AcpUpdate): void => emit("acp:update", update);
  const become = (to: AcpState, why: AcpError | null = null): void => {
    state = to;
    error = why;
    if (to !== "needsLogin") afterLogin = null;
    news(
      to === "needsLogin"
        ? { type: "status", state: to, error: why, methods: [...MOCK_LOGIN_METHODS] }
        : { type: "status", state: to, error: why },
    );
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
      case "login":
        stopTimer();
        afterLogin = now.then;
        become("needsLogin", now.why);
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
    const tool = (n: number, title: string, kind: AcpToolKind, status: AcpToolStatus): Step => ({
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

  function agents(): AgentEntry[] {
    const choice = probe.choice;
    const listed = MOCK_AGENTS.map((a) => ({
      ...a,
      chosen: choice !== null && "agent" in choice && choice.agent === a.id,
    }));
    if (choice === null || !("command" in choice)) return listed;
    return [
      ...listed,
      {
        id: "custom",
        // as the plugin answers: the window words an own command
        name: "",
        description: "",
        version: "",
        how: "custom",
        needs: null,
        command: choice.command,
        website: null,
        chosen: true,
      },
    ];
  }

  /** The chosen entry of the list (`null`: none chosen). */
  function chosen(): AgentEntry | null {
    return agents().find((a) => a.chosen) ?? null;
  }

  const refuse = (): void => {
    const fail = probe.fail;
    probe.fail = null;
    if (fail !== null) throw fail;
  };

  /** Each file's list entry before its last accept | reject (what undo puts back). */
  const undos = new Map<string, FileChange>();
  const listed = (): ChangeList => ({ files: probe.changes, more: 0 });

  function decide(path: string, hunks: string[] | null, accepted: boolean): ChangeList {
    refuse();
    const file = probe.changes.find((c) => c.path === path);
    if (file === undefined) throw new MockPluginError("notFound", path, "noChange");
    if (hunks !== null && hunks.length === 0)
      throw new MockPluginError("invalidData", path, "unknownHunk");
    if (file.kind === "conflict" && hunks !== null)
      throw new MockPluginError("conflict", path, "changed");
    if (hunks !== null && hunks.some((h) => !file.hunks.some((k) => k.key === h)))
      throw new MockPluginError("conflict", path, "outdated");
    // as the plugin: a link is only ever removed; a new | deleted | binary file only by every key of its list
    if (accepted && file.kind === "link") throw new MockPluginError("invalidData", path, "link");
    if (file.kind !== "modified" && hunks !== null && hunks.length !== file.hunks.length)
      throw new MockPluginError("invalidData", path, "wholeFileOnly");
    undos.set(path, file);
    if (hunks === null || file.kind !== "modified") {
      probe.changes = probe.changes.filter((c) => c !== file);
      return listed();
    }
    // an accepted hunk moves the real lines after it, as the plugin's fresh list says
    let shift = 0;
    const kept = file.hunks.flatMap((hunk) => {
      if (hunks.includes(hunk.key)) {
        if (accepted) shift += hunk.newLines.length - hunk.oldLines.length;
        return [];
      }
      return [{ ...hunk, oldStart: hunk.oldStart + shift }];
    });
    probe.changes = probe.changes.flatMap((c) =>
      c !== file ? [c] : kept.length === 0 ? [] : [{ ...c, hunks: kept }],
    );
    return listed();
  }

  function undo(path: string): ChangeList {
    refuse();
    const before = undos.get(path);
    if (before === undefined) throw new MockPluginError("notFound", path, "noUndo");
    undos.delete(path);
    probe.changes = [...probe.changes.filter((c) => c.path !== path), before].sort((a, b) =>
      a.path < b.path ? -1 : a.path > b.path ? 1 : 0,
    );
    return listed();
  }

  return {
    probe,
    handlers: {
      "acp.status": () => {
        probe.calls.push("status");
        const agent = chosen();
        return {
          state,
          agent: agent?.command ?? "",
          agentName: agent === null || agent.how === "custom" ? null : agent.name,
          chosen: agent !== null,
          error,
          methods: state === "needsLogin" ? [...MOCK_LOGIN_METHODS] : [],
          ...(probe.snap
            ? { needs: "snap" as const }
            : agent !== null && agent.needs !== null
              ? { needs: agent.needs }
              : {}),
          skippedFiles: probe.skipped,
        };
      },
      "acp.agents": () => {
        probe.calls.push("agents");
        return agents();
      },
      "acp.choose": (args) => {
        probe.calls.push("choose");
        if (probe.snap) throw new MockPluginError("unavailable", null, "snap");
        if (state === "starting" || state === "working")
          throw new MockPluginError("conflict", null, "busy");
        const command = args.command?.trim();
        if (args.agent !== undefined && command === undefined) {
          if (!MOCK_AGENTS.some((a) => a.id === args.agent))
            throw new MockPluginError("invalidData", args.agent, "unknownAgent");
          probe.choice = { agent: args.agent };
        } else if (args.agent === undefined && command !== undefined) {
          if (command === "") throw new MockPluginError("invalidData", null, "emptyCommand");
          // double quotes group a word: one left open is refused, as the plugin does
          if (command.split('"').length % 2 === 0)
            throw new MockPluginError("invalidData", null, "openQuote");
          if (relativeProgram(command))
            throw new MockPluginError("invalidData", null, "relativeProgram");
          probe.choice = { command };
        } else throw new MockPluginError("invalidData", null, "oneChoice");
        // as the plugin does: the agent stops, the next message starts the chosen one
        stopTurn();
        started = false;
        become("idle");
        return agents();
      },
      "acp.prompt": (sent) => {
        probe.calls.push("prompt");
        check(sent);
        if (sent.selection !== undefined && probe.hidden.has(sent.selection.path))
          throw new MockPluginError("denied", sent.selection.path, "hiddenFromAgents");
        // an open document agents may not see is not named
        const { document, ...rest } = sent;
        const args: PromptArgs =
          document === undefined || probe.hidden.has(document) ? rest : { ...rest, document };
        const agent = chosen();
        if (agent === null) throw new MockPluginError("unavailable", null, "noAgent");
        if (state === "starting" || state === "working")
          throw new MockPluginError("conflict", null, "busy");
        probe.prompts.push(structuredClone(args));
        const steps = script(args);
        // a prompt while a sign-in waits starts a fresh agent, as the plugin does
        if (started && state !== "needsLogin") become("working");
        else {
          started = true;
          become("starting");
          const working: Step = {
            kind: "news",
            update: { type: "status", state: "working", error: null },
          };
          steps.unshift(
            probe.signedOut.has(agent.id) ? { kind: "login", then: [working], why: null } : working,
          );
        }
        next = steps;
        run();
        return null;
      },
      "acp.login": (args) => {
        probe.calls.push("login");
        const then = afterLogin;
        if (state !== "needsLogin" || then === null)
          throw new MockPluginError("notFound", null, "noLogin");
        const method =
          args.methodId === undefined
            ? null
            : (MOCK_LOGIN_METHODS.find((m) => m.id === args.methodId) ?? null);
        if (args.methodId !== undefined && method === null)
          throw new MockPluginError("invalidData", args.methodId, "unknownMethod");
        probe.logins.push({ ...args });
        const agent = chosen();
        if (agent !== null && method?.kind === "agent") probe.signedOut.delete(agent.id);
        const signedIn = agent === null || !probe.signedOut.has(agent.id);
        // still signed out outside the app: the sign-in waits again, saying why
        next = signedIn
          ? [...then, ...next]
          : [{ kind: "login", then, why: MOCK_NOT_SIGNED_IN }, ...next];
        become("starting");
        run();
        return null;
      },
      "acp.cancel": () => {
        probe.calls.push("cancel");
        if (state === "needsLogin") {
          // as the plugin does: Stop while a sign-in waits stops the agent
          stopTurn();
          started = false;
          become("idle");
          return null;
        }
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
        return listed();
      },
      "acp.accept": ({ path, hunks }) => {
        probe.calls.push("accept");
        return decide(path, hunks, true);
      },
      "acp.reject": ({ path, hunks }) => {
        probe.calls.push("reject");
        return decide(path, hunks, false);
      },
      "acp.undo": ({ path }) => {
        probe.calls.push("undo");
        return undo(path);
      },
    },
  };
}

// What the assistant plugin answers in the mock backend: the agents to choose from (one in each state a computer can
// give: installed, started by npx | uvx, missing Node.js | uv | an install) and the person's choice (none at first, as
// on a first run), the agent's status, a timed stand-in reply to a message (status, thinking, text in pieces, tool
// lines, one question that waits for the person's answer, then the turn's end with one changed file) and the work
// copy's changes, where accept | reject drop hunks (named by key) and undo puts the file's last list entry back.
// Gemini CLI asks for a sign-in at its first start: its own method signs in, "Try again" alone stays signed out. The
// plugin's checks of what it is sent are mirrored (a passage alone is a message, line range, project paths, files
// taken only as a whole, links never accepted, relative program paths, inside a Snap nothing starts), so the window's
// words for them are tested against the same refusals. The project's last conversation is kept (one from an earlier
// run with Claude to start with; each new conversation takes its place) and continued: replayed by an agent that loads
// conversations (Claude, Gemini), remembered without a replay by one that only resumes them (Codex). A started agent
// offers its settings (a model, an effort level, a fast mode, as Claude's adapter does); a change is checked as the
// plugin checks it, answered by the new values and kept for that agent's next start. A finished reply
// renders through the mock note view, which draws headings, paragraphs, inline code and links only: lists, fenced code,
// tables and pictures stay their Markdown text here (check their look against the real renderer). Nothing is
// written: the real copy, agent, choice file and writes are the plugin's (tested there); the mock only feeds the
// window's flow.

import type { AcpConfigOption } from "./generated/AcpConfigOption";
import type { AcpConfigValue } from "./generated/AcpConfigValue";
import type { AcpError } from "./generated/AcpError";
import type { AcpState } from "./generated/AcpState";
import type { AcpToolKind } from "./generated/AcpToolKind";
import type { AcpToolStatus } from "./generated/AcpToolStatus";
import type { AcpUpdate } from "./generated/AcpUpdate";
import type { AgentEntry } from "./generated/AgentEntry";
import type { AnswerArgs } from "./generated/AnswerArgs";
import type { ChangeList } from "./generated/ChangeList";
import type { ContinueWay } from "./generated/ContinueWay";
import type { FileChange } from "./generated/FileChange";
import type { LoginArgs } from "./generated/LoginArgs";
import type { LoginMethod } from "./generated/LoginMethod";
import type { PermissionChoice } from "./generated/PermissionChoice";
import type { PromptArgs } from "./generated/PromptArgs";
import { mockNoteView } from "./mockDocs";
import { MockPluginError } from "./mockOrganizing";
import type { PluginEventName, PluginEvents, PluginMethods } from "./types";

type AcpMethods = Pick<
  PluginMethods,
  | "acp.status"
  | "acp.agents"
  | "acp.choose"
  | "acp.resolve"
  | "acp.login"
  | "acp.prompt"
  | "acp.cancel"
  | "acp.reset"
  | "acp.continue"
  | "acp.answer"
  | "acp.setOption"
  | "acp.changes"
  | "acp.accept"
  | "acp.reject"
  | "acp.undo"
  | "acp.render"
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

/** How each agent of the mock list continues a conversation (absent: it cannot). */
const MOCK_WAYS: Readonly<Record<string, ContinueWay>> = {
  "claude-acp": "load",
  gemini: "load",
  "codex-acp": "resume",
};

/** The project's last conversation as the mock keeps it: what its agent replays, and whether changes waited. */
export type MockSaved = {
  /** The agent's id, or the own command (`command:<text>`). */
  agent: string;
  title: string | null;
  at: number;
  supports: ContinueWay | null;
  /** The news a replay sends again (the person's messages, the replies, tool lines). */
  history: AcpUpdate[];
  hadChanges: boolean;
};

/** A conversation with Claude from an earlier run, which left a change the person did not accept. */
export function mockEarlierConversation(): MockSaved {
  return {
    agent: "claude-acp",
    title: "Clearer first steps in the guide",
    at: Date.now() - 2 * 60 * 60 * 1000,
    supports: "load",
    history: [
      { type: "you", text: "Make the first steps in the guide clearer." },
      {
        type: "tool",
        id: "earlier-1",
        title: "Read docs/guide.md",
        kind: "read",
        status: "completed",
      },
      { type: "text", text: "I split the first steps into two sentences.\n" },
    ],
    hadChanges: true,
  };
}

/** The settings a started agent offers, as Claude's adapter does: the model (default first), the effort, a fast mode. */
export const MOCK_OPTIONS: readonly AcpConfigOption[] = [
  {
    id: "model",
    name: "Model",
    description: "AI model to use",
    category: "model",
    kind: {
      type: "select",
      current: "default",
      choices: [
        { value: "default", name: "Default (recommended)", description: "Opus 5.5" },
        { value: "opus", name: "Opus 5.5", description: "For complex work and everyday tasks" },
        { value: "sonnet", name: "Sonnet 5", description: "For everyday tasks" },
        { value: "haiku", name: "Haiku 5", description: "Fastest for quick answers" },
      ],
    },
  },
  {
    id: "effort",
    name: "Effort",
    description: "Available effort levels for this model",
    category: "thoughtLevel",
    kind: {
      type: "select",
      current: "default",
      choices: [
        { value: "default", name: "Default", description: null },
        { value: "low", name: "Low", description: null },
        { value: "medium", name: "Medium", description: null },
        { value: "high", name: "High", description: null },
      ],
    },
  },
  {
    id: "fast",
    name: "Fast mode",
    description: "Faster responses on supported models",
    category: "modelConfig",
    kind: { type: "toggle", on: false },
  },
];

/** `options` with `value` set for option `id` (`null`: the option does not take it). */
function withValue(
  options: readonly AcpConfigOption[],
  id: string,
  value: AcpConfigValue,
): AcpConfigOption[] | null {
  const option = options.find((o) => o.id === id);
  if (option === undefined) return null;
  const kind = option.kind;
  if (kind.type === "select") {
    if (typeof value !== "string" || !kind.choices.some((c) => c.value === value)) return null;
    return options.map((o) => (o.id === id ? { ...o, kind: { ...kind, current: value } } : o));
  }
  if (typeof value !== "boolean") return null;
  return options.map((o) => (o.id === id ? { ...o, kind: { type: "toggle", on: value } } : o));
}

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
  /** The project's last conversation (`null`: none kept). */
  saved: MockSaved | null;
  /** The settings changes asked for, in order. */
  optionSets: { id: string; value: AcpConfigValue }[];
  /** The agent refuses the next settings change (with these words). */
  refuseOption: string | null;
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
          contextBefore: "",
          contextAfter: "",
        },
        {
          key: "guide-17",
          oldStart: 17,
          oldLines: ["| Ctrl+= / Ctrl+- | Zoom         |"],
          newLines: ["| Ctrl+= / Ctrl+- | Zoom         |", "| Ctrl+E       | Edit            |"],
          truncated: false,
          contextBefore: "| Alt+Left     | Back            |",
          contextAfter: null,
        },
      ],
    },
  ];
}

/** The end of a reply in pieces: what a finished message renders (a heading, a list, code, a table, links, a code
 *  reference, a picture on the web). */
const SUMMARY: readonly string[] = [
  "## What changed\n\n",
  "- the first steps, now two sentences\n- a new row for `Ctrl+E`\n\n",
  `See ${GUIDE}:3, the [guide](${GUIDE}) and [CommonMark](https://commonmark.org).\n\n`,
  "```sh\nnpm run check-links -- --all-files-in-the-project-including-drafts\n```\n\n",
  "| Shortcut | Action |\n|---|---|\n| Ctrl+E | Edit |\n\n",
  "![Link map](https://example.com/link-map.png)\n",
];

const OPTIONS: PermissionChoice[] = [
  { id: "allow-once", name: "Yes", kind: "allow_once" },
  { id: "allow-always", name: "Yes, and don't ask again", kind: "allow_always" },
  { id: "reject", name: "No", kind: "reject_once" },
];

/** One news of a reply, or the question the reply stops at until it is answered. */
type Step =
  | { kind: "news"; update: AcpUpdate }
  | { kind: "ask"; update: Extract<AcpUpdate, { type: "permission" }> }
  /** The agent wants a sign-in before the reply goes on (`then`: its first steps after it; `why`: a failed one). */
  | { kind: "login"; then: Step[]; why: AcpError | null }
  /** The continued conversation is ready: its next news is kept again. */
  | { kind: "resumed" }
  /** The started agent tells its settings (the person's picks for it applied). */
  | { kind: "options" }
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

/** Longest passage a prompt attaches, in UTF-8 bytes (the plugin's `MAX_PASSAGE_BYTES`). */
const MAX_PASSAGE_BYTES = 256 * 1024;

/** Longest message the plugin renders, in UTF-8 bytes (its `MAX_REPLY_BYTES`). */
const MAX_REPLY_BYTES = 256 * 1024;
/** The document a message is rendered as (the plugin's `REPLY_DOC`). */
const REPLY_DOC = "assistant.md";

/** Refuses a prompt the plugin would refuse before anything starts. */
function check(args: PromptArgs): void {
  if (args.text.trim() === "" && args.selection === undefined)
    throw new MockPluginError("invalidData", null, "emptyPrompt");
  const selection = args.selection;
  if (selection !== undefined) {
    projectFile(selection.path);
    if (selection.fromLine === 0 || selection.toLine < selection.fromLine)
      throw new MockPluginError("invalidData", selection.path, "lineRange");
    if (new TextEncoder().encode(selection.text).length > MAX_PASSAGE_BYTES)
      throw new MockPluginError("tooLarge", selection.path, "passageTooLarge");
  }
  if (args.document !== undefined) projectFile(args.document);
}

/** The command's program: its first word (double quotes group). */
function programOf(command: string): string {
  return command.startsWith('"')
    ? command.slice(1, command.indexOf('"', 1))
    : (command.split(/\s+/)[0] ?? "");
}

const isAbsolute = (program: string): boolean => /^([A-Za-z]:[\\/]|[\\/])/.test(program);

/** Why the plugin refuses an own command (trimmed), by its detail; `null`: it would be kept. */
function commandRefusal(command: string): string | null {
  if (command === "") return "emptyCommand";
  // double quotes group a word: one left open is refused, as the plugin does
  if (command.split('"').length % 2 === 0) return "openQuote";
  const program = programOf(command);
  if (program === "") return "emptyCommand";
  // two separators first: a share | device path, never looked up (a lookup alone would reach the server)
  if (/^[\\/]{2}/.test(program)) return "networkProgram";
  // a path with a folder that is not absolute would name a file under the folder the agent starts in
  if (/[\\/]/.test(program) && !isAbsolute(program)) return "relativeProgram";
  return null;
}

/** The programs the mock computer finds by their bare name (as `PATH` would). */
const MOCK_PROGRAMS: Readonly<Record<string, string>> = {
  npx: "C:\\Program Files\\nodejs\\npx.cmd",
  node: "C:\\Program Files\\nodejs\\node.exe",
  uvx: "C:\\Users\\you\\.local\\bin\\uvx.exe",
  "my-agent": "C:\\Users\\you\\.local\\bin\\my-agent.exe",
};

/** The full path of the program a kept own command would start; `null`: not found on the mock computer. */
function programFound(command: string): string | null {
  const program = programOf(command);
  return isAbsolute(program) ? program : (MOCK_PROGRAMS[program] ?? null);
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
    saved: mockEarlierConversation(),
    optionSets: [],
    refuseOption: null,
    stopAll: stopTimer,
  };
  /** The running agent's settings (empty: none runs | it has not told them yet). */
  let options: readonly AcpConfigOption[] = [];
  /** The person's picks, by agent. */
  const picks = new Map<string, Map<string, AcpConfigValue>>();
  /** The news of the conversation of the running agent is kept for a replay (`false`: none runs | it replays). */
  let recording = false;
  const news = (update: AcpUpdate): void => {
    if (recording && (update.type === "text" || update.type === "tool"))
      probe.saved?.history.push(update);
    emit("acp:update", update);
  };
  const become = (to: AcpState, why: AcpError | null = null): void => {
    state = to;
    error = why;
    // no agent, no settings (the window drops them with the status, as the plugin's news says)
    if (to === "idle" || to === "error") options = [];
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
      case "resumed":
        stopTimer();
        recording = true;
        become("ready");
        return;
      case "options": {
        const agent = chosen();
        let told: readonly AcpConfigOption[] = MOCK_OPTIONS;
        for (const [id, value] of (agent === null ? undefined : picks.get(agentKey(agent))) ?? [])
          told = withValue(told, id, value) ?? told;
        options = told;
        news({ type: "options", options: structuredClone([...told]) });
        return;
      }
      case "end":
        stopTimer();
        if (probe.changes.length === 0) probe.changes = guideChanges();
        if (probe.saved !== null) probe.saved.hadChanges = probe.changes.length > 0;
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
          reach: "outside",
          detail: "npm run check-links",
          summary: "Checks every link in the guide",
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
            ? "All links work. The changes wait for you below.\n\n"
            : "I did not run the check. The changes wait for you below.\n\n",
        },
      },
      ...SUMMARY.map((chunk): Step => ({ kind: "news", update: { type: "text", text: chunk } })),
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

  /** How a kept conversation names its agent. */
  const agentKey = (entry: AgentEntry): string =>
    entry.how === "custom" ? `command:${entry.command}` : entry.id;

  /** The kept conversation the chosen agent can continue (`null`: none). */
  function resumable(): MockSaved | null {
    const agent = chosen();
    const saved = probe.saved;
    if (agent === null || saved === null || saved.supports === null) return null;
    return saved.agent === agentKey(agent) ? saved : null;
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
        const saved = resumable();
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
          last:
            saved === null || saved.supports === null
              ? null
              : { title: saved.title, at: saved.at, supports: saved.supports },
          options: [...options],
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
          const refusal = commandRefusal(command);
          if (refusal !== null) throw new MockPluginError("invalidData", null, refusal);
          probe.choice = { command };
        } else throw new MockPluginError("invalidData", null, "oneChoice");
        // as the plugin does: the agent stops, the next message starts the chosen one
        stopTurn();
        started = false;
        become("idle");
        return agents();
      },
      "acp.resolve": (args) => {
        const command = args.command.trim();
        const error = probe.snap ? "snap" : commandRefusal(command);
        return { program: error === null ? programFound(command) : null, error };
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
          // a new conversation takes the place of the one kept
          probe.saved = {
            agent: agentKey(agent),
            title: args.text.trim() === "" ? null : args.text.trim().slice(0, 60),
            at: Date.now(),
            supports: agent.how === "custom" ? null : (MOCK_WAYS[agent.id] ?? null),
            history: [],
            hadChanges: false,
          };
          started = true;
          become("starting");
          steps.unshift({ kind: "options" });
          const working: Step = {
            kind: "news",
            update: { type: "status", state: "working", error: null },
          };
          steps.unshift(
            probe.signedOut.has(agent.id) ? { kind: "login", then: [working], why: null } : working,
          );
        }
        probe.saved?.history.push({ type: "you", text: args.text });
        recording = true;
        next = steps;
        run();
        return null;
      },
      "acp.continue": () => {
        probe.calls.push("continue");
        const agent = chosen();
        if (agent === null) throw new MockPluginError("unavailable", null, "noAgent");
        const saved = resumable();
        if (saved === null) throw new MockPluginError("notFound", null, "noSaved");
        if (state === "starting" || state === "working")
          throw new MockPluginError("conflict", null, "busy");
        stopTurn();
        recording = false;
        started = true;
        become("starting");
        // the copy of this run is still there: only one made anew loses the changes left in the last one
        const changesLost = saved.hadChanges && probe.changes.length === 0;
        saved.hadChanges = false;
        saved.at = Date.now();
        const replay: Step[] =
          saved.supports === "load"
            ? saved.history.map((update): Step => ({ kind: "news", update }))
            : [];
        next = [
          { kind: "options" },
          ...replay,
          {
            kind: "news",
            update: { type: "restored", replayed: saved.supports === "load", changesLost },
          },
          { kind: "resumed" },
        ];
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
        recording = false;
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
      "acp.setOption": (args) => {
        probe.calls.push("setOption");
        if (state === "starting") throw new MockPluginError("conflict", null, "starting");
        if (state !== "ready" && state !== "working")
          throw new MockPluginError("unavailable", null, "notRunning");
        if (!options.some((o) => o.id === args.id))
          throw new MockPluginError("notFound", args.id, "unknownSetting");
        const next = withValue(options, args.id, args.value);
        if (next === null) throw new MockPluginError("invalidData", args.id, "notOffered");
        probe.optionSets.push({ ...args });
        const refusal = probe.refuseOption;
        if (refusal !== null) {
          probe.refuseOption = null;
          // told again as they are (fresh, as from the backend), then why
          news({ type: "options", options: structuredClone([...options]) });
          news({ type: "status", state, error: { code: "optionRefused", message: refusal } });
          return null;
        }
        options = next;
        const agent = chosen();
        if (agent !== null) {
          const mine = picks.get(agentKey(agent)) ?? new Map<string, AcpConfigValue>();
          mine.set(args.id, args.value);
          picks.set(agentKey(agent), mine);
        }
        news({ type: "options", options: structuredClone(next) });
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
      "acp.render": ({ text }) => {
        probe.calls.push("render");
        if (new TextEncoder().encode(text).length > MAX_REPLY_BYTES)
          throw new MockPluginError("tooLarge", null, "replyTooLarge");
        // no renderer here: a heading | a paragraph per run of lines, inline code and links as elements
        const body = mockNoteView(REPLY_DOC, text, { inline: true }).body;
        return { blocks: body.kind === "markdown" ? body.blocks : [] };
      },
    },
  };
}

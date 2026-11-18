// The assistant of the project as this window shows it: the agent's state (the backend's word), the conversation so
// far (kept while the window shows the project; "New conversation" clears it; the newest `MAX_ENTRIES` lines only),
// the work copy's changes waiting for the person, and the passage attached to the next message. Events of the project
// shown only: another project starts afresh, and an answer that comes back after the switch is dropped. The passage
// follows the selection in the open document; a selection | the keyboard inside the panel leaves it as it is, so
// clicking into the message box keeps what was selected. Which agent runs is the person's choice (kept by the backend
// for every project): none chosen, the picker stands in for the conversation. What changes for the person (the agent
// answers, waits for an answer, is done) is said on the window's one polite status line, never by the growing text.

import type {
  AcpError,
  AcpErrorCode,
  AcpState,
  AcpStatus,
  AcpStopReason,
  AcpToolStatus,
  AcpUpdate,
  AgentEntry,
  ApiError,
  ChangeList,
  ChooseArgs,
  FileChange,
  IpcError,
  LoginMethod,
  PermissionChoice,
  PromptArgs,
  PromptSelection,
} from "../../api";
import { documents } from "../../core/doc/document.svelte";
import { pageSelection } from "../../core/doc/selection";
import { editing, type TextRange } from "../../core/editing/session.svelte";
import { apiErrorText } from "../../core/errors";
import { t, tn, type MessageKey } from "../../core/i18n";
import { PluginClient, pluginClient, workspaceRoot } from "../../core/plugins/client";
import {
  type Arrangement,
  groupOf,
  intoGroup,
  OUTLINE,
  regionOf,
} from "../../core/shell/arrangement";
import { arrangement } from "../../core/shell/arrangement.svelte";
import { showView, togglePanel } from "../../core/shell/sides";
import { windowStore, type NoticeAction } from "../../core/window.svelte";

/** The id of the Assistant panel. */
export const PANEL = "acp.panel";
/** The attribute on the panel's element: a selection | the keyboard inside it never changes the passage. */
export const PANEL_MARK = "data-acp-panel";
/** The conversation keeps this many lines; older ones go (a long session never grows the window without end). */
export const MAX_ENTRIES = 500;
/** The agent's error output shown under Details: its newest characters, at most this many. */
const MAX_DETAILS = 4000;
/** A selection being dragged is read at most once per this many ms (reading it measures the whole edit text). */
const CAPTURE_MS = 100;

/** The answer to one of the agent's questions, as the card shows it. */
export type Answer =
  | { status: "open" }
  | { status: "answered"; option: string }
  /** The turn ended (Stop, a failure) before an answer: the agent no longer waits. */
  | { status: "closed" };

/** Where a tool line stands: the agent's word, or `stopped` (the turn ended before the tool did). */
export type ToolState = AcpToolStatus | "stopped";

/** One line of the conversation. */
export type Entry =
  | { kind: "you"; id: number; text: string; passage: PromptSelection | null }
  | { kind: "agent"; id: number; text: string }
  | { kind: "tool"; id: number; tool: string; title: string; status: ToolState }
  | {
      kind: "question";
      id: number;
      requestId: string;
      title: string;
      detail: string;
      options: PermissionChoice[];
      answer: Answer;
    }
  /** Something the person should know (a stop, a refusal, a failure); `details`: the agent's error output. */
  | { kind: "notice"; id: number; text: string; details: string | null }
  /** Why the agent still waits for a sign-in: gone with that agent when the person chooses another. */
  | { kind: "signIn"; id: number; text: string; details: string | null }
  /** Another agent took over: it does not know the lines above. */
  | { kind: "divider"; id: number; name: string };

/** An entry before it gets its id. */
type NewEntry = Entry extends infer E ? (E extends Entry ? Omit<E, "id"> : never) : never;

/** The agents the person can choose, as the backend lists them for this computer. */
export type Catalog =
  | { status: "unread" }
  | { status: "loading" }
  | { status: "ready"; agents: readonly AgentEntry[] }
  | { status: "failed"; text: string };

/** What the store tells the window beyond its own state (tests give their own). */
export type AcpDeps = {
  /** Says a line on the window's polite status line. */
  say: (text: string) => void;
  notices: { notifyKeyed(key: string, text: string, ...actions: NoticeAction[]): void };
};

export class AcpStore {
  status = $state.raw<AcpStatus | null>(null);
  transcript = $state.raw<readonly Entry[]>([]);
  /** Lines dropped from the start of the conversation (past `MAX_ENTRIES`). */
  hidden = $state(0);
  /** The agent thinks: one indicator after the lines, until it says | does something. */
  thinking = $state(false);
  changes = $state.raw<readonly FileChange[]>([]);
  /** Changed files past the listed ones (the backend lists at most 500). */
  moreChanges = $state(0);
  /** The passage the next message carries (`null`: none). */
  passage = $state.raw<PromptSelection | null>(null);
  /** The message being written (kept while the view moves between regions). */
  draft = $state("");
  /** A message is on its way to the backend. */
  sending = $state(false);
  /** The file an accept | reject is running for (`null`: none). */
  deciding = $state<string | null>(null);
  /** Why the last accept | reject | undo of a file was refused. */
  refusal = $state.raw<{ path: string; text: string } | null>(null);
  catalog = $state.raw<Catalog>({ status: "unread" });
  /** The person asked to change the agent (none chosen: the picker shows anyway). */
  picking = $state(false);
  /** A choice is on its way to the backend. */
  choosing = $state(false);
  /** Why the last choice was refused (`null`: none). */
  choiceRefusal = $state<string | null>(null);
  /** A sign-in is on its way to the backend. */
  signingIn = $state(false);

  /** A question of the agent waits for the person. */
  readonly asking = $derived(
    this.transcript.some((e) => e.kind === "question" && e.answer.status === "open"),
  );

  #ids = 0;
  /** Reads a selection change still waiting for its moment now (`null`: none waits). */
  #flush: (() => void) | null = null;
  readonly #client: PluginClient;
  readonly #root: () => string | null;
  readonly #deps: AcpDeps;

  constructor(
    client: PluginClient = pluginClient,
    root: () => string | null = workspaceRoot,
    deps: Partial<AcpDeps> = {},
  ) {
    this.#client = client;
    this.#root = root;
    this.#deps = {
      say: deps.say ?? ((text) => arrangement.say(text)),
      notices: deps.notices ?? windowStore,
    };
  }

  /** The agent starts | works, or a message is on its way: nothing new can be sent. */
  get busy(): boolean {
    const state = this.status?.state;
    return this.sending || state === "starting" || state === "working";
  }

  /** The agent can be changed now: not while it starts | works (the backend refuses then). */
  get canChange(): boolean {
    return !this.busy;
  }

  /** The picker shows: no agent chosen yet, the person asked to change it, or nothing can start here (Snap). */
  get showsPicker(): boolean {
    const status = this.status;
    return status !== null && (!status.chosen || this.picking || status.needs === "snap");
  }

  /** Follows the project of the window and the selection in its document until the returned teardown runs. */
  start(): () => void {
    return $effect.root(() => {
      $effect(() => {
        if (this.#root() === null) return;
        let gone = false;
        const offs: (() => void)[] = [];
        const keep = (stop: () => void): void => {
          if (gone) stop();
          else offs.push(stop);
        };
        void this.#client.on("acp:update", (update) => this.take(update)).then(keep);
        void this.#client.call("acp.status", {}).then((result) => {
          if (!gone && result.ok) this.#took(result.value);
        });
        void this.refreshChanges();
        return () => {
          gone = true;
          for (const off of offs) off();
          // a new project has its own agent: nothing of this one's conversation | changes stands for it
          this.clear();
        };
      });
      $effect(() => {
        // the editors take the selection from the same event: read once they have, then at most once per moment
        let timer: ReturnType<typeof setTimeout> | null = null;
        let last = -CAPTURE_MS;
        const run = (): void => {
          if (timer !== null) clearTimeout(timer);
          timer = null;
          this.#flush = null;
          last = Date.now();
          this.capture();
        };
        const read = (): void => {
          if (timer !== null) return;
          timer = setTimeout(run, Math.max(0, last + CAPTURE_MS - Date.now()));
          this.#flush = run;
        };
        document.addEventListener("selectionchange", read);
        return () => {
          document.removeEventListener("selectionchange", read);
          if (timer !== null) clearTimeout(timer);
          this.#flush = null;
        };
      });
    });
  }

  /** Reads a selection change still waiting now: the pointer goes into the panel, where it would not count. */
  flushCapture(): void {
    this.#flush?.();
  }

  /** Takes one news of the agent. */
  take(update: AcpUpdate): void {
    switch (update.type) {
      case "status": {
        const before = this.status?.state;
        this.status = {
          agent: this.status?.agent ?? "",
          agentName: this.status?.agentName ?? null,
          chosen: this.status?.chosen ?? false,
          state: update.state,
          error: update.error,
          methods: update.methods ?? [],
          needs: this.status?.needs,
          skippedFiles: this.status?.skippedFiles ?? 0,
        };
        if (update.state !== "starting" && update.state !== "working") {
          this.thinking = false;
          this.#stopOpen();
        }
        if (update.error !== null)
          this.#push({
            kind: update.state === "needsLogin" ? "signIn" : "notice",
            text: failureText(update.error),
            details: detailsOf(update.error),
          });
        if (update.state !== before || update.error !== null)
          this.#sayState(update.state, update.error);
        // the agent stopped (a choice in any window of the app among the causes): its name is read again
        if (update.state === "idle") void this.#readStatus();
        return;
      }
      case "text": {
        this.thinking = false;
        const last = this.transcript.at(-1);
        if (last?.kind === "agent")
          this.transcript = [
            ...this.transcript.slice(0, -1),
            { ...last, text: last.text + update.text },
          ];
        else this.#push({ kind: "agent", text: update.text });
        return;
      }
      case "thinking":
        this.thinking = true;
        return;
      case "tool": {
        this.thinking = false;
        const line = { tool: update.id, title: update.title, status: update.status };
        const at = this.transcript.findIndex((e) => e.kind === "tool" && e.tool === update.id);
        if (at < 0) this.#push({ kind: "tool", ...line });
        else this.transcript = this.transcript.map((e, i) => (i === at ? { ...e, ...line } : e));
        return;
      }
      case "permission":
        this.thinking = false;
        this.#push({
          kind: "question",
          requestId: update.requestId,
          title: update.title,
          detail: update.detail,
          options: update.options,
          answer: { status: "open" },
        });
        this.#deps.say(t("acp.say.waiting"));
        return;
      case "permissionDone":
        this.#answer(update.requestId, { status: "closed" }, true);
        return;
      case "turnEnd": {
        this.thinking = false;
        // the protocol asks the client to mark what the turn left unfinished: the agent will not finish it
        this.#stopOpen();
        const said = endKey(update.stopReason, update.changed > 0);
        if (said !== null) this.#push({ kind: "notice", text: t(said), details: null });
        const done = endSaid(update.stopReason);
        if (done !== null)
          this.#deps.say(
            update.changed > 0
              ? t("acp.say.withChanges", {
                  said: done,
                  changes: tn("acp.say.changes", update.changed),
                })
              : done,
          );
        if (update.changed > 0 || this.changes.length > 0) void this.refreshChanges();
        // the copy was brought up to date for this turn: what it left out may have changed
        void this.#readStatus();
        return;
      }
      default:
        update satisfies never;
    }
  }

  /** Sends the message being written with the attached passage and the open document; the reply comes as news. */
  async send(): Promise<void> {
    const text = this.draft;
    if (text.trim() === "" || this.busy) return;
    const passage = this.passage;
    const document = documents.path;
    const args: PromptArgs = {
      text,
      ...(passage === null ? {} : { selection: passage }),
      ...(document === null ? {} : { document }),
    };
    this.#push({ kind: "you", text, passage });
    this.draft = "";
    this.passage = null;
    this.sending = true;
    const root = this.#root();
    try {
      const result = await this.#client.call("acp.prompt", args);
      if (result.ok || this.#root() !== root) return;
      this.#notice(t("acp.notSent", { reason: errorText(result.error) }));
      // another window may have changed the choice: the picker | the name follows what the backend holds
      if (result.error.kind === "backend" && result.error.error.detail === "noAgent")
        void this.#readStatus();
      // nothing was sent: the message and its passage come back unless the person wrote | selected anew
      if (this.draft === "") this.draft = text;
      this.passage ??= passage;
    } finally {
      if (this.#root() === root) this.sending = false;
    }
  }

  /** Stops the turn in progress; a refused Stop is said. */
  async cancel(): Promise<void> {
    const root = this.#root();
    const result = await this.#client.call("acp.cancel", {});
    if (!result.ok && this.#root() === root) this.#notice(errorText(result.error));
  }

  /** A new conversation: the agent starts afresh with the next message; the changes stay. */
  async newConversation(): Promise<void> {
    const root = this.#root();
    const result = await this.#client.call("acp.reset", {});
    if (this.#root() !== root) return;
    if (!result.ok) {
      this.#notice(errorText(result.error));
      return;
    }
    this.transcript = [];
    this.hidden = 0;
    this.thinking = false;
    this.refusal = null;
  }

  /** Answers the question `requestId` with `option`. */
  async answer(requestId: string, option: PermissionChoice): Promise<void> {
    // a restarted agent numbers its questions afresh: an earlier card of the same id keeps its own answer
    this.#answer(requestId, { status: "answered", option: option.name }, true);
    const root = this.#root();
    const result = await this.#client.call("acp.answer", { requestId, optionId: option.id });
    if (result.ok || this.#root() !== root) return;
    this.#answer(requestId, { status: "closed" });
    this.#notice(errorText(result.error));
  }

  /** Reads the work copy's changes again (`refresh`: the backend computes them anew). An answer that comes after the
   *  window turned to another project is dropped. */
  async refreshChanges(refresh = false): Promise<void> {
    const root = this.#root();
    const result = await this.#client.call("acp.changes", refresh ? { refresh } : {});
    if (this.#root() !== root || !result.ok) return;
    this.#list(result.value);
  }

  #list(list: ChangeList): void {
    this.changes = list.files;
    this.moreChanges = list.more;
    // a refusal about a file no longer listed has nothing left to stand beside
    const refused = this.refusal?.path;
    if (refused !== undefined && !list.files.some((f) => f.path === refused)) this.refusal = null;
  }

  /** Accepts (into the real file) | rejects (from the copy) the hunks keyed `hunks` of `path` (`null`: the whole
   *  file as it is now). Done: one notice per file says it, with Undo. */
  async decide(choice: "accept" | "reject", path: string, hunks: string[] | null): Promise<void> {
    if (this.deciding !== null) return;
    this.deciding = path;
    this.refusal = null;
    const root = this.#root();
    try {
      const result = await this.#client.call(choice === "accept" ? "acp.accept" : "acp.reject", {
        path,
        hunks,
      });
      if (this.#root() !== root) return;
      if (result.ok) {
        this.#list(result.value);
        this.#deps.notices.notifyKeyed(
          `acp.decided:${path}`,
          t(choice === "accept" ? "acp.done.accepted" : "acp.done.rejected", { path }),
          { label: t("fileOps.undo"), run: () => this.undo(path) },
        );
        if (result.value.files.length === 0) this.#deps.say(t("acp.say.allHandled"));
        return;
      }
      this.refusal = { path, text: errorText(result.error) };
      if (result.error.kind === "backend" && outdated(result.error.error))
        await this.refreshChanges(true);
    } finally {
      if (this.#root() === root) this.deciding = null;
    }
  }

  /** Takes back the last accept | reject of `path` (once); a refusal is said beside the list. */
  async undo(path: string): Promise<void> {
    const root = this.#root();
    const result = await this.#client.call("acp.undo", { path });
    if (this.#root() !== root) return;
    if (result.ok) {
      this.refusal = null;
      this.#list(result.value);
      this.#deps.say(t("acp.say.undone", { path }));
    } else this.refusal = { path, text: errorText(result.error) };
  }

  /** Takes the passage selected in the open document as the one to attach (none selected: none). A selection | the
   *  keyboard inside the panel changes nothing. */
  capture(): void {
    if (inPanel(document.activeElement) || inPanel(document.getSelection()?.anchorNode ?? null))
      return;
    this.passage = selectedPassage();
  }

  /** The header's Change, the palette command: the panel shows the picker. */
  openPicker(): void {
    showView(PANEL);
    this.picking = true;
    this.choiceRefusal = null;
  }

  /** Back to the conversation without a new choice. */
  closePicker(): void {
    this.picking = false;
    this.choiceRefusal = null;
  }

  /** Reads the agents the person can choose again (an install since the last read shows). The list read before stays
   *  on screen meanwhile. */
  async loadAgents(): Promise<void> {
    const root = this.#root();
    if (this.catalog.status !== "ready") this.catalog = { status: "loading" };
    const result = await this.#client.call("acp.agents", {});
    if (this.#root() !== root) return;
    this.catalog = result.ok
      ? { status: "ready", agents: result.value }
      : { status: "failed", text: errorText(result.error) };
  }

  /** Keeps the person's choice: the agent stops, the chosen one starts with the next message. Resolves to false when
   *  the choice was refused (said in `choiceRefusal`). */
  async choose(choice: ChooseArgs): Promise<boolean> {
    if (this.choosing) return false;
    this.choosing = true;
    this.choiceRefusal = null;
    const root = this.#root();
    try {
      const result = await this.#client.call("acp.choose", choice);
      if (this.#root() !== root) return false;
      if (!result.ok) {
        this.choiceRefusal = errorText(result.error);
        return false;
      }
      this.catalog = { status: "ready", agents: result.value };
      this.picking = false;
      // the backend stopped the last agent: the sign-in it waited for is gone with it
      this.transcript = this.transcript.filter((e) => e.kind !== "signIn");
      if (this.status?.state === "needsLogin")
        this.status = { ...this.status, state: "idle", error: null, methods: [] };
      // the new agent's name read back draws the line between the conversations
      await this.#readStatus();
      return true;
    } finally {
      if (this.#root() === root) this.choosing = false;
    }
  }

  /** Signs in while the agent waits for it: an `agent` method through the agent; `null` | any other method: the
   *  person signed in outside, the agent is asked again. The outcome comes as news. */
  async login(method: LoginMethod | null): Promise<void> {
    if (this.signingIn) return;
    this.signingIn = true;
    const root = this.#root();
    try {
      const result = await this.#client.call(
        "acp.login",
        method?.kind === "agent" ? { methodId: method.id } : {},
      );
      if (!result.ok && this.#root() === root) this.#notice(errorText(result.error));
    } finally {
      if (this.#root() === root) this.signingIn = false;
    }
  }

  /** A website in the person's browser (how to install an agent | what it needs). */
  openWebsite(url: string): void {
    void documents.openWeb(url);
  }

  /** Opens a file of the project at a line (the real file, as it is now). */
  openAt(path: string, line: number | null): void {
    documents.navigate(path, null, line);
  }

  /** Puts `text` on the clipboard; false when the system refused. */
  async copy(text: string): Promise<boolean> {
    try {
      await navigator.clipboard.writeText(text);
      return true;
    } catch {
      return false;
    }
  }

  /** Removes the attached passage. */
  dropPassage(): void {
    this.passage = null;
  }

  /** The palette command: the panel shows | closes. */
  toggle(): void {
    togglePanel(PANEL);
  }

  /** Forgets everything of the project shown (another project, the plugin switched off), calls on their way too:
   *  what they answer late is dropped. */
  clear(): void {
    this.status = null;
    this.transcript = [];
    this.hidden = 0;
    this.thinking = false;
    this.changes = [];
    this.moreChanges = 0;
    this.passage = null;
    this.draft = "";
    this.sending = false;
    this.deciding = null;
    this.refusal = null;
    this.catalog = { status: "unread" };
    this.picking = false;
    this.choosing = false;
    this.choiceRefusal = null;
    this.signingIn = false;
  }

  /** Reads the chosen agent again (its state comes as news). */
  async #readStatus(): Promise<void> {
    const root = this.#root();
    const result = await this.#client.call("acp.status", {});
    if (this.#root() === root && result.ok) this.#took(result.value);
  }

  /** Takes an answer of `acp.status`: news that came meanwhile is newer, so only the chosen agent is taken then. Another
   *  agent than the one before (a choice in any window) draws a line under the conversation so far. */
  #took(answer: AcpStatus): void {
    const now = this.status;
    if (
      now?.chosen === true &&
      answer.chosen &&
      (now.agent !== answer.agent || now.agentName !== answer.agentName) &&
      this.transcript.length > 0
    )
      this.#push({ kind: "divider", name: answer.agentName ?? t("acp.ownCommand") });
    this.status =
      now === null
        ? answer
        : {
            ...now,
            agent: answer.agent,
            agentName: answer.agentName,
            chosen: answer.chosen,
            needs: answer.needs,
            skippedFiles: answer.skippedFiles,
          };
  }

  /** Says what a new state means for the person: a failure in words, else the states that ask something of them
   *  (the states they wait through say nothing). */
  #sayState(state: AcpState, error: AcpError | null): void {
    if (error !== null) {
      this.#deps.say(failureText(error));
      return;
    }
    switch (state) {
      case "working":
        this.#deps.say(t("acp.say.working"));
        return;
      case "needsLogin":
        this.#deps.say(t("acp.say.signIn"));
        return;
      case "error":
      case "idle":
      case "starting":
      case "ready":
        return;
      default:
        state satisfies never;
    }
  }

  /** Tool lines the agent left pending | running: it will not finish them. Questions still open: the backend answered
   *  each one by now (the card of one answered while it was on its way would else stay open). */
  #stopOpen(): void {
    const open = (e: Entry): boolean =>
      (e.kind === "tool" && (e.status === "pending" || e.status === "in_progress")) ||
      (e.kind === "question" && e.answer.status === "open");
    if (!this.transcript.some(open)) return;
    this.transcript = this.transcript.map((e) => {
      if (!open(e)) return e;
      return e.kind === "question"
        ? { ...e, answer: { status: "closed" } }
        : e.kind === "tool"
          ? { ...e, status: "stopped" }
          : e;
    });
  }

  #notice(text: string): void {
    this.#push({ kind: "notice", text, details: null });
  }

  #push(entry: NewEntry): void {
    this.#ids += 1;
    const next = [...this.transcript, { ...entry, id: this.#ids }];
    const over = next.length - MAX_ENTRIES;
    if (over > 0) this.hidden += over;
    this.transcript = over > 0 ? next.slice(over) : next;
  }

  /** Sets the answer of question `requestId`; `onlyOpen`: a question already answered keeps its answer. */
  #answer(requestId: string, answer: Answer, onlyOpen = false): void {
    this.transcript = this.transcript.map((e) =>
      e.kind === "question" &&
      e.requestId === requestId &&
      (!onlyOpen || e.answer.status === "open")
        ? { ...e, answer }
        : e,
    );
  }
}

/**
 * Where the panel goes the first time it shows: a tab of the outline's group, selected, instead of a group of its own
 * that splits the right column (a conversation needs the height). `null`: the person arranged it before (any region
 * lists it), or the outline is not in the right column: the default place stands.
 */
export function firstPlace(arr: Arrangement): Arrangement | null {
  if (regionOf(arr, PANEL) !== null || regionOf(arr, OUTLINE) !== "right") return null;
  const group = groupOf(arr.right, OUTLINE);
  const tabs = arr.right[group]?.views.length ?? 0;
  return intoGroup(arr, PANEL, "right", group, tabs);
}

/** Lines (1-based) of `text` that `range` covers; a range ending right after a line break ends on that line. */
export function linesOf(text: string, range: TextRange): [number, number] {
  const lineAt = (offset: number): number => {
    let line = 1;
    for (let at = text.indexOf("\n"); at >= 0 && at < offset; at = text.indexOf("\n", at + 1))
      line += 1;
    return line;
  };
  const end = range.to > range.from && text[range.to - 1] === "\n" ? range.to - 1 : range.to;
  return [lineAt(range.from), lineAt(Math.max(range.from, end))];
}

/** The passage selected in the document on screen (`null`: none, or its lines are not known). */
function selectedPassage(): PromptSelection | null {
  const path = documents.path;
  if (path === null) return null;
  if (editing.mode === "view") {
    const page = pageSelection();
    if (page === null || page.lines === null || page.text.trim() === "") return null;
    return { path, fromLine: page.lines[0], toLine: page.lines[1], text: page.text };
  }
  // while another file loads, the editor on screen is still the previous file's
  if (editing.path !== path) return null;
  const range = editing.scope("selection");
  const text = editing.selection;
  if (range === null || text.trim() === "") return null;
  // the edit text keeps the file's lines: Source is the file's text, Edit writes untouched blocks with their own bytes
  const [fromLine, toLine] = linesOf(editing.current(), range);
  return { path, fromLine, toLine, text };
}

function inPanel(node: Node | null): boolean {
  const element = node instanceof Element ? node : (node?.parentElement ?? null);
  return element?.closest(`[${PANEL_MARK}]`) != null;
}

/** What the chip | a sent message says of a passage: its document and lines. */
export function passageLabel(passage: PromptSelection): string {
  return passage.fromLine === passage.toLine
    ? t("acp.passageLine", { path: passage.path, line: passage.fromLine })
    : t("acp.passage", { path: passage.path, from: passage.fromLine, to: passage.toLine });
}

/** The first words of a passage on one line (at most `max` characters), to say what is attached. */
export function passageStart(passage: PromptSelection, max = 60): string {
  const line = passage.text.replace(/\s+/g, " ").trim();
  return line.length <= max ? line : `${line.slice(0, max - 1).trimEnd()}…`;
}

/** The agent can start as listed (installed, or its runner found). */
export function canStart(entry: AgentEntry): boolean {
  return entry.how !== "missing" && entry.needs === null;
}

/** Registry ids the picker lists first, in this order (those this computer's list has). */
const POPULAR: readonly string[] = [
  "claude-acp",
  "codex-acp",
  "github-copilot-cli",
  "gemini",
  "opencode",
  "goose",
  "cursor",
];

/** The picker's groups: the person's own command (when chosen), the popular agents, all the others A-Z. */
export type PickerGroups = {
  own: AgentEntry[];
  popular: AgentEntry[];
  all: AgentEntry[];
};

/** The agents whose name, id, description (an own command: its words too) hold every word of `query`, in groups. */
export function pickerList(agents: readonly AgentEntry[], query: string): PickerGroups {
  const words = query
    .toLocaleLowerCase()
    .split(/\s+/)
    .filter((w) => w !== "");
  const found = agents.filter((agent) => {
    const command = agent.how === "custom" ? agent.command : "";
    const text =
      `${entryName(agent)} ${agent.id} ${agent.description} ${command}`.toLocaleLowerCase();
    return words.every((word) => text.includes(word));
  });
  const listed = found.filter((agent) => agent.how !== "custom");
  const rank = (agent: AgentEntry): number => POPULAR.indexOf(agent.id);
  return {
    own: found.filter((agent) => agent.how === "custom"),
    popular: listed.filter((agent) => rank(agent) >= 0).sort((a, b) => rank(a) - rank(b)),
    all: listed
      .filter((agent) => rank(agent) < 0)
      .sort((a, b) => a.name.localeCompare(b.name, undefined, { sensitivity: "base" })),
  };
}

/** How the picker | a notice names an agent: the registry's name; the person's own command in words. */
export function entryName(entry: AgentEntry): string {
  return entry.how === "custom" ? t("acp.ownCommand") : entry.name;
}

/** What the chip of a listed agent says: how it would start, else what it needs first. */
export function howKey(entry: AgentEntry): MessageKey {
  switch (entry.how) {
    case "native":
      return "acp.how.native";
    case "npx":
      return "acp.how.npx";
    case "uvx":
      return "acp.how.uvx";
    case "custom":
      return entry.needs === null ? "acp.how.custom" : "acp.how.notFound";
    case "missing":
      switch (entry.needs) {
        case "node":
          return "acp.needs.node";
        case "uv":
          return "acp.needs.uv";
        case "install":
        case null:
          return "acp.needs.install";
        case "snap":
          return "acp.how.notFound";
        default:
          return entry.needs satisfies never;
      }
    default:
      return entry.how satisfies never;
  }
}

/** Why an agent that cannot start cannot, and the page that helps (`url` null: the agent's own website, if any). */
export function whyNot(entry: AgentEntry): {
  why: MessageKey;
  help: MessageKey;
  url: string | null;
} {
  const lack = entry.how === "custom" ? "custom" : entry.needs;
  switch (lack) {
    case "node":
      return { why: "acp.why.node", help: "acp.pick.installNode", url: NODE_URL };
    case "uv":
      return { why: "acp.why.uv", help: "acp.pick.installUv", url: UV_URL };
    case "custom":
    case "snap":
      return { why: "acp.why.notFound", help: "acp.pick.howToInstall", url: null };
    case "install":
    case null:
      return { why: "acp.why.install", help: "acp.pick.howToInstall", url: null };
    default:
      return lack satisfies never;
  }
}

/** Where Node.js | uv tell how to install them (opened only on the person's click, in their browser). */
const NODE_URL = "https://nodejs.org/en/download";
const UV_URL = "https://docs.astral.sh/uv/getting-started/installation/";

/** What the turn's end says in the conversation (`null`: nothing; a failure says itself in the status news);
 *  `changed`: the copy holds changes the person can still review. */
function endKey(stopReason: AcpStopReason, changed: boolean): MessageKey | null {
  switch (stopReason) {
    case "cancelled":
      return changed ? "acp.stoppedChanges" : "acp.stopped";
    case "refusal":
      return "acp.declined";
    case "max_tokens":
    case "max_turn_requests":
      return "acp.cutShort";
    case "end_turn":
    case "error":
    case "other":
      return null;
    default:
      return stopReason satisfies never;
  }
}

/** What the window announces at the turn's end (`null`: a failure, which the status news says). */
function endSaid(stopReason: AcpStopReason): string | null {
  switch (stopReason) {
    case "end_turn":
    case "other":
      return t("acp.say.done");
    case "error":
      return null;
    case "cancelled":
    case "refusal":
    case "max_tokens":
    case "max_turn_requests":
      return t(endKey(stopReason, false) ?? "acp.say.done");
    default:
      return stopReason satisfies never;
  }
}

function failKey(code: AcpErrorCode): MessageKey {
  switch (code) {
    case "noAgent":
      return "acp.fail.noAgent";
    case "notFound":
      return "acp.fail.notFound";
    case "noProfile":
      return "acp.fail.noProfile";
    case "copyFailed":
      return "acp.fail.copyFailed";
    case "tooLarge":
      return "acp.fail.tooLarge";
    case "projectsNotReady":
      return "acp.fail.projectsNotReady";
    case "startFailed":
      return "acp.fail.startFailed";
    case "timedOut":
      return "acp.fail.timedOut";
    case "initFailed":
      return "acp.fail.initFailed";
    case "protocolVersion":
      return "acp.fail.protocolVersion";
    case "sessionFailed":
      return "acp.fail.sessionFailed";
    case "loginFailed":
      return "acp.fail.loginFailed";
    case "stillNeedsLogin":
      return "acp.fail.stillNeedsLogin";
    case "refused":
      return "acp.fail.refused";
    case "protocol":
      return "acp.fail.protocol";
    case "exited":
      return "acp.fail.exited";
    case "sendFailed":
      return "acp.fail.sendFailed";
    default:
      return code satisfies never;
  }
}

/** What went wrong with the agent, in words; the agent's own words after them when it gave some. */
export function failureText(error: AcpError): string {
  const reason = t(failKey(error.code));
  const message = error.message?.trim() ?? "";
  return message === "" ? reason : t("acp.fail.said", { reason, message });
}

/** The agent's error output with a failure (its newest part; `null`: none): shown apart, under Details. */
function detailsOf(error: AcpError): string | null {
  const text = error.stderr?.trim() ?? "";
  if (text === "") return null;
  return text.length <= MAX_DETAILS ? text : text.slice(-MAX_DETAILS);
}

/** The plugin's refusals in words; other errors as the app words them. */
export function errorText(error: ApiError): string {
  const key = error.kind === "backend" ? refusalKey(error.error) : null;
  return key === null ? apiErrorText(error) : t(key);
}

function refusalKey(error: IpcError): MessageKey | null {
  switch (error.detail) {
    case "busy":
      return "acp.error.busy";
    case "emptyPrompt":
      return "acp.error.emptyPrompt";
    case "lineRange":
      return "acp.error.lineRange";
    case "noQuestion":
    case "unknownOption":
      return "acp.error.noQuestion";
    case "unsaved":
      return "acp.error.unsaved";
    case "changed":
      return "acp.error.changed";
    case "exists":
      return "acp.error.exists";
    case "wholeFileOnly":
      return "acp.error.wholeFileOnly";
    case "unknownHunk":
    case "outdated":
    case "noChange":
    case "noWorkCopy":
      return "acp.error.outdated";
    case "noAgent":
      return "acp.error.noAgent";
    case "unknownAgent":
      return "acp.error.unknownAgent";
    case "emptyCommand":
      return "acp.error.emptyCommand";
    case "openQuote":
      return "acp.error.openQuote";
    case "relativeProgram":
      return "acp.error.relativeProgram";
    case "oneChoice":
      return "acp.error.oneChoice";
    case "noProfile":
      return "acp.error.noProfile";
    case "noLogin":
      return "acp.error.noLogin";
    case "unknownMethod":
      return "acp.error.unknownMethod";
    case "denied":
      return "acp.error.denied";
    case "link":
      return "acp.error.link";
    case "linkInPath":
      return "acp.error.linkInPath";
    case "noUndo":
      return "acp.error.noUndo";
    case "hiddenFromAgents":
      return "acp.error.hiddenFromAgents";
    case "snap":
      return "acp.snap";
    case "insideProject":
      return "acp.error.insideProject";
    case "scanCutOff":
    case "tooManyFiles":
    case "tooManyBytes":
      return "acp.fail.tooLarge";
    case "projectsNotReady":
      return "acp.fail.projectsNotReady";
    case "noText":
    case "edits":
      return "acp.error.notApplied";
    default:
      return error.kind === "invalidPath" || error.kind === "pathEscape" ? "acp.error.path" : null;
  }
}

/** The list the refusal was about is old: read it again. */
function outdated(error: IpcError): boolean {
  return ["unknownHunk", "outdated", "noChange", "noWorkCopy", "wholeFileOnly", "changed"].includes(
    error.detail ?? "",
  );
}

export const acp = new AcpStore();

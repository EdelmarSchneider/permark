// The assistant of the project as this window shows it: the agent's state (the backend's word), the conversation so
// far (kept while the window shows the project; "New conversation" clears it), the work copy's changes waiting for
// the person, and the passage attached to the next message. Events of the project shown only: another project starts
// afresh. The passage follows the selection in the open document; a selection | the keyboard inside the panel leaves
// it as it is, so clicking into the message box keeps what was selected.

import type {
  AcpStatus,
  AcpUpdate,
  ApiError,
  FileChange,
  IpcError,
  PermissionChoice,
  PromptArgs,
  PromptSelection,
} from "../../api";
import { documents } from "../../core/doc/document.svelte";
import { pageSelection } from "../../core/doc/selection";
import { editing, type TextRange } from "../../core/editing/session.svelte";
import { apiErrorText } from "../../core/errors";
import { t, type MessageKey } from "../../core/i18n";
import { PluginClient, pluginClient, workspaceRoot } from "../../core/plugins/client";
import { togglePanel } from "../../core/shell/sides";

/** The id of the Assistant panel. */
export const PANEL = "acp.panel";
/** The attribute on the panel's element: a selection | the keyboard inside it never changes the passage. */
export const PANEL_MARK = "data-acp-panel";

/** The answer to one of the agent's questions, as the card shows it. */
export type Answer =
  | { status: "open" }
  | { status: "answered"; option: string }
  /** The turn ended (Stop, a failure) before an answer: the agent no longer waits. */
  | { status: "closed" };

/** One line of the conversation. */
export type Entry =
  | { kind: "you"; id: number; text: string; passage: PromptSelection | null }
  | { kind: "agent"; id: number; text: string }
  | { kind: "thinking"; id: number }
  | { kind: "tool"; id: number; tool: string; title: string; status: string }
  | {
      kind: "question";
      id: number;
      requestId: string;
      title: string;
      detail: string;
      options: PermissionChoice[];
      answer: Answer;
    }
  /** Something the person should know (a stop, a refusal, the agent's own error text). */
  | { kind: "notice"; id: number; text: string };

/** An entry before it gets its id. */
type NewEntry = Entry extends infer E ? (E extends Entry ? Omit<E, "id"> : never) : never;

export class AcpStore {
  status = $state.raw<AcpStatus | null>(null);
  transcript = $state.raw<readonly Entry[]>([]);
  changes = $state.raw<readonly FileChange[]>([]);
  /** The passage the next message carries (`null`: none). */
  passage = $state.raw<PromptSelection | null>(null);
  /** The message being written (kept while the view moves between regions). */
  draft = $state("");
  /** A message is on its way to the backend. */
  sending = $state(false);
  /** The file an accept | reject is running for (`null`: none). */
  deciding = $state<string | null>(null);
  /** Why the last accept | reject of a file was refused. */
  refusal = $state.raw<{ path: string; text: string } | null>(null);
  #ids = 0;
  readonly #client: PluginClient;
  readonly #root: () => string | null;

  constructor(client: PluginClient = pluginClient, root: () => string | null = workspaceRoot) {
    this.#client = client;
    this.#root = root;
  }

  /** The agent starts | works, or a message is on its way: nothing new can be sent. */
  get busy(): boolean {
    const state = this.status?.state;
    return this.sending || state === "starting" || state === "working";
  }

  /** Follows the project of the window and the selection in its document until the returned teardown runs. */
  start(): () => void {
    return $effect.root(() => {
      $effect(() => {
        // a new project has its own agent: nothing of the last one's conversation | changes stands for it
        this.clear();
        if (this.#root() === null) return;
        let gone = false;
        const offs: (() => void)[] = [];
        const keep = (stop: () => void): void => {
          if (gone) stop();
          else offs.push(stop);
        };
        void this.#client.on("acp:update", (update) => this.take(update)).then(keep);
        void this.#client.call("acp.status", {}).then((result) => {
          if (gone || !result.ok) return;
          // news that came meanwhile is newer than this answer: only the command is taken from it then
          this.status =
            this.status === null ? result.value : { ...this.status, agent: result.value.agent };
        });
        void this.refreshChanges();
        return () => {
          gone = true;
          for (const off of offs) off();
        };
      });
      $effect(() => {
        // the editors take the selection from the same event: read once they have
        let timer: ReturnType<typeof setTimeout> | null = null;
        const read = (): void => {
          timer ??= setTimeout(() => {
            timer = null;
            this.capture();
          }, 0);
        };
        document.addEventListener("selectionchange", read);
        return () => {
          document.removeEventListener("selectionchange", read);
          if (timer !== null) clearTimeout(timer);
        };
      });
    });
  }

  /** Takes one news of the agent. */
  take(update: AcpUpdate): void {
    switch (update.type) {
      case "status":
        this.status = {
          agent: this.status?.agent ?? "",
          state: update.state,
          error: update.error,
        };
        if (update.error !== null) this.#push({ kind: "notice", text: update.error });
        return;
      case "text": {
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
        if (this.transcript.at(-1)?.kind !== "thinking") this.#push({ kind: "thinking" });
        return;
      case "tool": {
        const line = { tool: update.id, title: update.title, status: update.status };
        const at = this.transcript.findIndex((e) => e.kind === "tool" && e.tool === update.id);
        if (at < 0) this.#push({ kind: "tool", ...line });
        else this.transcript = this.transcript.map((e, i) => (i === at ? { ...e, ...line } : e));
        return;
      }
      case "permission":
        this.#push({
          kind: "question",
          requestId: update.requestId,
          title: update.title,
          detail: update.detail,
          options: update.options,
          answer: { status: "open" },
        });
        return;
      case "permissionDone":
        this.#answer(update.requestId, { status: "closed" }, true);
        return;
      case "turnEnd": {
        const said = endKey(update.stopReason);
        if (said !== null) this.#push({ kind: "notice", text: t(said) });
        if (update.changed > 0 || this.changes.length > 0) void this.refreshChanges();
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
    try {
      const result = await this.#client.call("acp.prompt", args);
      if (result.ok) return;
      this.#push({ kind: "notice", text: t("acp.notSent", { reason: errorText(result.error) }) });
      // nothing was sent: the message and its passage come back unless the person wrote | selected anew
      if (this.draft === "") this.draft = text;
      this.passage ??= passage;
    } finally {
      this.sending = false;
    }
  }

  /** Stops the turn in progress. */
  async cancel(): Promise<void> {
    await this.#client.call("acp.cancel", {});
  }

  /** A new conversation: the agent starts afresh with the next message; the changes stay. */
  async newConversation(): Promise<void> {
    const result = await this.#client.call("acp.reset", {});
    if (!result.ok) return;
    this.transcript = [];
    this.refusal = null;
  }

  /** Answers the question `requestId` with `option`. */
  async answer(requestId: string, option: PermissionChoice): Promise<void> {
    this.#answer(requestId, { status: "answered", option: option.name });
    const result = await this.#client.call("acp.answer", { requestId, optionId: option.id });
    if (result.ok) return;
    this.#answer(requestId, { status: "closed" });
    this.#push({ kind: "notice", text: errorText(result.error) });
  }

  /** Reads the work copy's changes again. An answer that comes after the window turned to another project is dropped. */
  async refreshChanges(): Promise<void> {
    const root = this.#root();
    const result = await this.#client.call("acp.changes", {});
    if (this.#root() !== root || !result.ok) return;
    this.changes = result.value;
  }

  /** Accepts (into the real file) | rejects (from the copy) hunks `hunks` of `path` (`null`: the whole file). */
  async decide(choice: "accept" | "reject", path: string, hunks: number[] | null): Promise<void> {
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
        this.changes = result.value;
        return;
      }
      this.refusal = { path, text: errorText(result.error) };
      if (result.error.kind === "backend" && outdated(result.error.error))
        await this.refreshChanges();
    } finally {
      this.deciding = null;
    }
  }

  /** Takes the passage selected in the open document as the one to attach (none selected: none). A selection | the
   *  keyboard inside the panel changes nothing. */
  capture(): void {
    if (inPanel(document.activeElement) || inPanel(document.getSelection()?.anchorNode ?? null))
      return;
    this.passage = selectedPassage();
  }

  /** Removes the attached passage. */
  dropPassage(): void {
    this.passage = null;
  }

  /** The palette command: the panel shows | closes. */
  toggle(): void {
    togglePanel(PANEL);
  }

  /** Forgets everything of the project shown (another project, the plugin switched off). */
  clear(): void {
    this.status = null;
    this.transcript = [];
    this.changes = [];
    this.passage = null;
    this.draft = "";
    this.refusal = null;
  }

  #push(entry: NewEntry): void {
    this.#ids += 1;
    this.transcript = [...this.transcript, { ...entry, id: this.#ids }];
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

/** A short name for the agent command: its program or package, without a version or a folder. */
export function agentLabel(command: string): string {
  const words = command.trim().split(/\s+/);
  const named = words.filter((w) => w !== "" && !w.startsWith("-")).at(-1) ?? command.trim();
  const unversioned = named.replace(/(.)@[^@/]*$/, "$1");
  const name = unversioned.split(/[\\/]/).at(-1) ?? unversioned;
  return name.replace(/\.(exe|cmd|bat|js|mjs)$/i, "");
}

/** What the turn's end says (`null`: nothing; a failure says itself in the status news). */
function endKey(stopReason: string): MessageKey | null {
  switch (stopReason) {
    case "cancelled":
      return "acp.stopped";
    case "refusal":
      return "acp.declined";
    case "max_tokens":
    case "max_turn_requests":
      return "acp.cutShort";
    default:
      return null;
  }
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
    case "noChange":
    case "noWorkCopy":
      return "acp.error.outdated";
    default:
      return error.kind === "invalidPath" || error.kind === "pathEscape" ? "acp.error.path" : null;
  }
}

/** The list the refusal was about is old: read it again. */
function outdated(error: IpcError): boolean {
  return ["unknownHunk", "noChange", "noWorkCopy", "wholeFileOnly", "changed"].includes(
    error.detail ?? "",
  );
}

export const acp = new AcpStore();

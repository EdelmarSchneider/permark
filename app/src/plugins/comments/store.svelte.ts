// Comments on passages. The backend keeps them and finds every passage again after an edit (`comments.json`, anchors);
// this mirrors them for the page (a bar in the margin), the Comments tab and the field, and sends the person's edits
// back. The text of a comment is the person's; nothing here touches the document.

import type { ApiError, CommentView } from "../../api";
import { documents } from "../../core/doc/document.svelte";
import { apiErrorText } from "../../core/errors";
import { t, tn } from "../../core/i18n";
import { PluginClient, pluginClient, workspaceRoot } from "../../core/plugins/client";
import type { PageSelection } from "../../core/plugins/props";
import type { TreeBadge } from "../../core/plugins/registry.svelte";
import { windowStore } from "../../core/window.svelte";
import {
  agentCommentPrompt,
  agentCommentsPrompt,
  barComments,
  doneComments,
  headingPathAt,
  openByDocument,
  openCounts,
} from "./model";

/** The events of a burst (a re-anchoring after a save) become one reload. */
export const RELOAD_MS = 120;
/** The longest comment, in characters. */
export const MAX_CHARS = 2000;

/** A comment being written: the passage it is about. */
export type Draft = {
  path: string;
  quote: string;
  headingPath: readonly string[];
  line: number;
  endLine: number;
  /** Where the passage is on screen (the field goes under it). */
  rect: DOMRect | null;
};

export class CommentsStore {
  all = $state.raw<readonly CommentView[]>([]);
  ready = $state(false);
  draft = $state.raw<Draft | null>(null);
  /** The Comments tab is asked to show a comment (it scrolls to it and marks it for a moment). */
  focus = $state.raw<{ id: string; n: number } | null>(null);
  readonly #client: PluginClient;
  readonly #root: () => string | null;
  readonly #notify: (text: string) => void;
  readonly #copy: (text: string) => Promise<boolean>;
  #timer: ReturnType<typeof setTimeout> | null = null;
  #focusCount = 0;
  /** The Comments tab shows one project's comments (its detail card's count led here); `null`: all. */
  only = $state.raw<{ folder: string; name: string } | null>(null);
  /** What the Comments tab lists: all | the project's (documents at | below its folder). */
  shown = $derived.by(() => {
    const only = this.only;
    if (only === null) return this.all;
    const prefix = `${only.folder}/`;
    return this.all.filter((c) => c.path.startsWith(prefix));
  });
  groups = $derived(openByDocument(this.shown));
  done = $derived(doneComments(this.shown));
  counts = $derived(openCounts(this.all));
  /** Open comments. */
  count = $derived(this.all.filter((c) => !c.done).length);

  constructor(
    client: PluginClient = pluginClient,
    root: () => string | null = workspaceRoot,
    notify: (text: string) => void = (text) => windowStore.notifyNote(text),
    copy: (text: string) => Promise<boolean> = copyText,
  ) {
    this.#client = client;
    this.#root = root;
    this.#notify = notify;
    this.#copy = copy;
  }

  /** Follows the project of the window until the returned teardown runs. */
  start(): () => void {
    return $effect.root(() => {
      $effect(() => {
        if (this.#root() === null) {
          this.#reset();
          return;
        }
        let off: (() => void) | null = null;
        let gone = false;
        void this.#client
          .on("comments:changed", () => this.#schedule())
          .then((stop) => {
            if (gone) stop();
            else off = stop;
          });
        void this.load();
        return () => {
          gone = true;
          off?.();
          this.#cancel();
        };
      });
      return () => this.#reset();
    });
  }

  #reset(): void {
    this.#cancel();
    this.all = [];
    this.ready = false;
    this.draft = null;
    this.focus = null;
    this.only = null;
  }

  #cancel(): void {
    if (this.#timer !== null) clearTimeout(this.#timer);
    this.#timer = null;
  }

  #schedule(): void {
    if (this.#timer !== null) return;
    this.#timer = setTimeout(() => {
      this.#timer = null;
      void this.load();
    }, RELOAD_MS);
  }

  async load(): Promise<void> {
    const root = this.#root();
    const result = await this.#client.call("comments.list", { scope: "all" });
    if (this.#root() !== root) return;
    if (result.ok) {
      this.all = result.value;
      this.ready = true;
    }
  }

  // ---- the page -----------------------------------------------------------------------------------------------

  /** The comments that get a bar in the margin of the document shown. */
  bars(path: string | null): CommentView[] {
    return barComments(this.all, path);
  }

  /** A count on the row of a document that has open comments. */
  badge(path: string, isDir: boolean): TreeBadge | null {
    if (isDir) return null;
    const n = this.counts.get(path) ?? 0;
    return n === 0
      ? null
      : { text: String(n), tone: "muted", title: t("comments.badgeHint", { n }) };
  }

  /** Open comments on documents at | below a folder (a project's detail card); `null` when there are none. */
  projectCount(folder: string): string | null {
    const prefix = `${folder}/`;
    const n = this.all.filter((c) => !c.done && c.path.startsWith(prefix)).length;
    return n === 0 ? null : tn("comments.projectCount", n);
  }

  /** The Comments tab lists only the comments of a project's folder (`null`: all again). */
  showOnly(only: { folder: string; name: string } | null): void {
    this.only = only;
  }

  // ---- writing ------------------------------------------------------------------------------------------------

  /** Starts a comment on the selected passage of the page. `false`: there is nothing to comment on. */
  begin(selection: PageSelection): boolean {
    const path = documents.path;
    const view = documents.view;
    const quote = selection.text.trim();
    if (path === null || quote === "" || view?.body.kind !== "markdown") return false;
    const blockOf = (node: Node | null): string | null => {
      const element = node instanceof Element ? node : (node?.parentElement ?? null);
      return element?.closest<HTMLElement>("[data-b]")?.getAttribute("data-b") ?? null;
    };
    const first = blockOf(selection.node);
    const last = blockOf(selection.endNode) ?? first;
    const blocks = view.body.blocks;
    const lines = [first, last].flatMap((id) => {
      const block = blocks.find((b) => b.id === id);
      return block ? [block] : [];
    });
    const start = Math.min(...lines.map((b) => b.lineStart));
    const end = Math.max(...lines.map((b) => b.lineEnd));
    this.draft = {
      path,
      quote,
      headingPath: first === null ? [] : headingPathAt(blocks, view.body.outline, first),
      line: Number.isFinite(start) ? start : 1,
      endLine: Number.isFinite(end) ? end : 1,
      rect: selection.rect,
    };
    return true;
  }

  cancel(): void {
    this.draft = null;
  }

  /** Saves the comment being written. `true` when it is kept (the field closes). */
  async save(text: string): Promise<boolean> {
    const draft = this.draft;
    if (draft === null || text.trim() === "") return false;
    const result = await this.#client.call("comments.add", {
      path: draft.path,
      quote: draft.quote,
      text: text.trim().slice(0, MAX_CHARS),
      headingPath: [...draft.headingPath],
      line: draft.line,
      endLine: draft.endLine,
    });
    if (!result.ok) {
      this.#fail(result.error);
      return false;
    }
    if (this.draft === draft) this.draft = null;
    await this.load();
    return true;
  }

  async setDone(id: string, done: boolean): Promise<void> {
    const result = await this.#client.call("comments.setDone", { id, done });
    if (!result.ok) this.#fail(result.error);
    await this.load();
  }

  async remove(id: string): Promise<void> {
    const result = await this.#client.call("comments.remove", { id });
    if (!result.ok) this.#fail(result.error);
    await this.load();
  }

  async reply(id: string, text: string): Promise<boolean> {
    if (text.trim() === "") return false;
    const result = await this.#client.call("comments.reply", { id, text });
    if (!result.ok) {
      this.#fail(result.error);
      return false;
    }
    await this.load();
    return true;
  }

  /** Asks the tab to show a comment. */
  show(id: string): void {
    this.focus = { id, n: ++this.#focusCount };
  }

  /** Goes to the passage of a comment (the document opens at its line). */
  jump(comment: CommentView): void {
    if (comment.state === "documentMissing") {
      this.#notify(t("comments.jump.gone", { path: comment.path }));
      return;
    }
    documents.navigate(comment.path, null, comment.line);
  }

  // ---- send to agent ------------------------------------------------------------------------------------------

  /** One open comment as a prompt, on the clipboard, ready to paste into an agent's chat. */
  async sendToAgent(id: string): Promise<void> {
    const comment = this.all.find((c) => c.id === id && !c.done);
    if (comment) await this.#send(agentCommentPrompt(comment));
  }

  /** The same for every open comment, grouped by document. */
  async sendAllToAgent(): Promise<void> {
    if (this.count === 0) return;
    await this.#send(agentCommentsPrompt(this.groups));
  }

  async #send(text: string): Promise<void> {
    this.#notify(t((await this.#copy(text)) ? "comments.sent" : "comments.copyFailed"));
  }

  #fail(error: ApiError): void {
    this.#notify(t("comments.failed", { message: apiErrorText(error) }));
  }
}

async function copyText(text: string): Promise<boolean> {
  try {
    await navigator.clipboard.writeText(text);
    return true;
  } catch {
    return false;
  }
}

export const comments = new CommentsStore();

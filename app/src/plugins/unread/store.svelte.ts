// Unread markers like new mail: the documents that changed since the person read them, a dot on each, a count on the
// folders above, the next one a key away. The backend keeps the truth (a hash per document); this mirrors it.

import type { SinceVisit, UnreadList } from "../../api";
import { documents } from "../../core/doc/document.svelte";
import { InFront, onScreen } from "../../core/inFront.svelte";
import { PluginClient, pluginClient, workspaceRoot } from "../../core/plugins/client";
import { READ_MS, personScrolls } from "../../core/readRule";
import type { TreeBadge } from "../../core/plugins/registry.svelte";
import { t, tn } from "../../core/i18n";
import { silence } from "../../core/silence/silence.svelte";

const MARKDOWN = /\.(md|markdown|mdown|mkd|mdx|mdc)$/i;

export function isDocument(path: string): boolean {
  return MARKDOWN.test(path);
}

/** The folders above a path (`a/b/c.md` -> `a`, `a/b`). */
function ancestors(path: string): string[] {
  const parts = path.split("/");
  const out: string[] = [];
  for (let at = 1; at < parts.length; at++) out.push(parts.slice(0, at).join("/"));
  return out;
}

/**
 * The unread document after `from` in the list's order (newest change first), wrapping; the first one (the newest)
 * when `from` is not in the list; `null`: none other than `from`.
 */
export function nextUnread(unread: readonly string[], from: string | null): string | null {
  const at = from === null ? -1 : unread.indexOf(from);
  for (let step = 1; step <= unread.length; step++) {
    const path = unread[(at + step) % unread.length];
    if (path !== undefined && path !== from) return path;
  }
  return null;
}

/** The document the window shows and the version of it (reactive). */
export type ReadingPort = () => { path: string | null; hash: string | null };

const shownDocument: ReadingPort = () => {
  const view = documents.view;
  const path = documents.path;
  return { path, hash: view !== null && view.path === path ? view.hash : null };
};

export class UnreadStore {
  since = $state.raw<SinceVisit | null>(null);
  /** What the backend says: every unread document, silenced ones included, newest change first. */
  #list = $state.raw<UnreadList>({ ready: false, unread: [] });
  /** The ones that call for attention: what the person silenced is left out (the backend still keeps it unread). */
  unread = $derived.by(() => this.#list.unread.filter((path) => !silence.isSilenced(path)));
  /** The Unread tab shows one project's documents (its detail card's count led here); `null`: all. */
  only = $state.raw<{ folder: string; name: string } | null>(null);
  /** What the Unread tab lists: all | the project's (documents at | below its folder). */
  shown = $derived.by(() => {
    const only = this.only;
    if (only === null) return this.unread;
    const prefix = `${only.folder}/`;
    return this.unread.filter((p) => p.startsWith(prefix));
  });
  #all = $derived(new Set(this.#list.unread));
  #folders = $derived.by(() => {
    const folders = new Map<string, number>();
    for (const path of this.unread) {
      for (const folder of ancestors(path)) folders.set(folder, (folders.get(folder) ?? 0) + 1);
    }
    return folders;
  });
  /** The window is in front and visible, and this view is its tab on screen: reading counts only then. */
  readonly #front: InFront;
  readonly #client: PluginClient;
  readonly #root: () => string | null;
  readonly #shown: ReadingPort;

  constructor(
    client: PluginClient = pluginClient,
    root: () => string | null = workspaceRoot,
    shown: ReadingPort = shownDocument,
    onScreenNow: () => boolean = onScreen,
  ) {
    this.#client = client;
    this.#root = root;
    this.#shown = shown;
    this.#front = new InFront(onScreenNow);
  }

  get ready(): boolean {
    return this.#list.ready;
  }

  /** Follows the project of the window until the returned teardown runs. */
  start(): () => void {
    // what was silenced and now speaks starts from now: its old changes do not arrive at once
    const offWake = silence.onWake((paths) => {
      for (const path of paths) void this.markFolderRead(path);
    });
    return $effect.root(() => {
      $effect(() => {
        if (this.#root() === null) {
          this.#apply({ ready: false, unread: [] });
          this.since = null;
          return;
        }
        let off: (() => void) | null = null;
        let gone = false;
        void this.#client
          .on("unread:changed", (list) => {
            this.#apply(list);
            if (list.ready && this.since?.ready !== true) void this.#loadSince();
          })
          .then((stop) => {
            if (gone) stop();
            else off = stop;
          });
        void this.#client.call("unread.list", {}).then((result) => {
          if (gone || !result.ok) return;
          this.#apply(result.value);
          if (result.value.ready) void this.#loadSince();
        });
        return () => {
          gone = true;
          off?.();
        };
      });
      // reading: the document in view, still the same version a moment later, counts as read
      $effect(() => {
        const { path, hash } = this.#shown();
        if (path === null || hash === null) return;
        if (!this.#all.has(path) || !this.#front.value) return;
        const timer = setTimeout(() => void this.markRead(path, hash), READ_MS);
        return () => clearTimeout(timer);
      });
      // ...and sooner when the person scrolls it
      $effect(() => personScrolls(() => this.#scrolled()));
      $effect(() => this.#front.start());
      return () => {
        offWake();
        this.#apply({ ready: false, unread: [] });
        this.since = null;
      };
    });
  }

  /** The person scrolled the document in view: read, as it is shown (only in front, like the moment). */
  #scrolled(): void {
    const { path, hash } = this.#shown();
    if (path === null || hash === null) return;
    if (this.#all.has(path) && this.#front.value) void this.markRead(path, hash);
  }

  #apply(list: UnreadList): void {
    this.#list = list;
  }

  async #loadSince(): Promise<void> {
    const result = await this.#client.call("unread.sinceLastVisit", {});
    if (result.ok) this.since = result.value;
  }

  /** Whether the backend holds the document unread (silenced or not: reading it counts). */
  isUnread(path: string): boolean {
    return this.#all.has(path);
  }

  /** Unread documents below a folder. */
  countBelow(folder: string): number {
    return this.#folders.get(folder) ?? 0;
  }

  /** A dot on an unread document, a count on a folder with some inside. */
  badge(path: string, isDir: boolean): TreeBadge | null {
    if (isDir) {
      const n = this.#folders.get(path) ?? 0;
      return n === 0
        ? null
        : { text: String(n), tone: "unread", title: tn("unread.folderCount", n) };
    }
    return this.#all.has(path) && !silence.isSilenced(path)
      ? { text: "•", tone: "unread", title: t("unread.dot") }
      : null;
  }

  next(from: string | null): string | null {
    return nextUnread(this.unread, from);
  }

  /** The Unread tab lists only a project's documents (`null`: all again). */
  showOnly(only: { folder: string; name: string } | null): void {
    this.only = only;
  }

  /** Unread documents at | below a project's folder, for its detail card (`null` when none). */
  projectCount(folder: string): string | null {
    const n = this.countBelow(folder);
    return n === 0 ? null : tn("unread.folderCount", n);
  }

  async markRead(path: string, hash: string): Promise<void> {
    await this.#client.call("unread.markRead", { path, hash });
  }

  async markUnread(path: string): Promise<void> {
    await this.#client.call("unread.markUnread", { path });
  }

  /** A folder (`""`: everything) counts as read as it stands. */
  async markFolderRead(folder: string): Promise<void> {
    await this.#client.call("unread.markFolderRead", { folder });
  }
}

export const unread = new UnreadStore();

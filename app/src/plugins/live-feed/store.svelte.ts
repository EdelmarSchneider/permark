// The live feed: every file the project's disk reported as created, changed, removed or renamed, newest first, and
// the quiet toast that says documents changed while the person reads something else. Beside it, the person's own
// actions from the project's activity journal (and the hand-off marks) for the panel's mine | outside | all filter.

import type { ActivityEntry, FeedItem } from "../../api";
import { documents } from "../../core/doc/document.svelte";
import { PluginClient, pluginClient, workspaceRoot } from "../../core/plugins/client";
import { silence } from "../../core/silence/silence.svelte";
import { panelRows, pathOf, type FeedShow, type PanelRow } from "./mine";

/** Items kept (the backend keeps as many). */
const MAX_ITEMS = 500;
/** How long a toast stays. */
export const TOAST_MS = 6000;
/** A change this recent means the agent is working. */
export const ACTIVE_MS = 5000;

const MARKDOWN = /\.(md|markdown|mdown|mkd|mdx|mdc)$/i;
const TEXT_DOC = /\.(md|markdown|mdown|mkd|mdx|mdc|txt|text|rst|adoc|asciidoc|org)$/i;

export function isDocument(path: string): boolean {
  return MARKDOWN.test(path);
}

/** Whether the "Docs only" filter lets an item through: documents and groups (the group says how many docs). */
export function isDocItem(item: FeedItem): boolean {
  switch (item.item) {
    case "entry":
      return !item.isDir && TEXT_DOC.test(item.path);
    case "group":
    case "resync":
    case "project":
      return true;
    default:
      return item satisfies never;
  }
}

/** Newest first; an item with a known id replaces the one it updates. */
export function mergeItems(items: readonly FeedItem[], upsert: readonly FeedItem[]): FeedItem[] {
  const byId = new Map(items.map((i) => [i.id, i]));
  for (const item of upsert) byId.set(item.id, item);
  return [...byId.values()].sort((a, b) => b.atMs - a.atMs || b.id - a.id).slice(0, MAX_ITEMS);
}

export type Toast = {
  /** Documents that changed since the toast came up. */
  count: number;
  /** The newest of them. */
  path: string;
  kind: "created" | "modified" | "removed" | "renamed";
};

export class FeedStore {
  items = $state.raw<readonly FeedItem[]>([]);
  /** The person's own actions and the hand-off marks, oldest first. */
  mine = $state.raw<readonly ActivityEntry[]>([]);
  /** Which rows the panel lists. */
  show = $state<FeedShow>("all");
  docsOnly = $state(false);
  toast = $state.raw<Toast | null>(null);
  /** The time of the newest change (ms; 0: none yet). */
  lastAt = $state(0);
  readonly #client: PluginClient;
  readonly #root: () => string | null;
  readonly #shown: () => string | null;
  #timer: ReturnType<typeof setTimeout> | null = null;

  constructor(
    client: PluginClient = pluginClient,
    root: () => string | null = workspaceRoot,
    shown: () => string | null = () => documents.path,
  ) {
    this.#client = client;
    this.#root = root;
    this.#shown = shown;
  }

  /** The items the panel lists: what the person silenced is not among them. */
  get visible(): readonly FeedItem[] {
    const heard = this.items.filter(
      (item) => item.item !== "entry" || !silence.isSilenced(item.path),
    );
    return this.docsOnly ? heard.filter(isDocItem) : heard;
  }

  /** The panel's rows for the filter, newest first ("Docs only" applies to the person's actions too). */
  get rows(): PanelRow[] {
    const mine = this.docsOnly
      ? this.mine.filter((e) => {
          const path = pathOf(e.fact);
          return path === null || TEXT_DOC.test(path);
        })
      : this.mine;
    return panelRows(this.visible, mine, this.show);
  }

  /** How many items the "Docs only" filter hides. */
  get hidden(): number {
    if (!this.docsOnly) return 0;
    return (
      this.items.filter((i) => i.item !== "entry" || !silence.isSilenced(i.path)).length -
      this.visible.length
    );
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
        let offMine: (() => void) | null = null;
        void this.#client
          .on("live-feed:items", (data) => this.take(data.upsert))
          .then((stop) => {
            if (gone) stop();
            else off = stop;
          });
        void this.#client
          .on("live-feed:activity", (data) => this.takeMine(data.entry))
          .then((stop) => {
            if (gone) stop();
            else offMine = stop;
          });
        void this.#client.call("live-feed.items", {}).then((result) => {
          if (!gone && result.ok) this.#set(result.value);
        });
        void this.#client.call("live-feed.activity", {}).then((result) => {
          if (!gone && result.ok) this.mine = result.value.slice(-MAX_ITEMS);
        });
        return () => {
          gone = true;
          off?.();
          offMine?.();
        };
      });
      return () => this.#reset();
    });
  }

  #set(items: readonly FeedItem[]): void {
    this.items = mergeItems([], items);
    this.lastAt = this.items[0]?.atMs ?? 0;
  }

  #reset(): void {
    this.items = [];
    this.mine = [];
    this.lastAt = 0;
    this.dismissToast();
  }

  /** Items the backend reported: listed, and the document ones that changed behind the person's back say so. */
  take(upsert: readonly FeedItem[]): void {
    this.items = mergeItems(this.items, upsert);
    const heard = upsert.filter((i) => i.item !== "entry" || !silence.isSilenced(i.path));
    this.lastAt = Math.max(this.lastAt, ...heard.map((i) => i.atMs));
    const news = heard.filter(
      (i): i is Extract<FeedItem, { item: "entry" }> =>
        i.item === "entry" && !i.own && !i.isDir && isDocument(i.path) && i.path !== this.#shown(),
    );
    const latest = news[0];
    if (!latest) return;
    const kind = latest.kind;
    this.toast = {
      count: (this.toast?.count ?? 0) + news.length,
      path: latest.path,
      kind,
    };
    this.#arm();
  }

  /** One of the person's actions (or a hand-off) the journal just noted. */
  takeMine(entry: ActivityEntry): void {
    if (this.mine.some((e) => e.seq === entry.seq)) return;
    this.mine = [...this.mine, entry].slice(-MAX_ITEMS);
  }

  #arm(): void {
    if (this.#timer !== null) clearTimeout(this.#timer);
    this.#timer = setTimeout(() => this.dismissToast(), TOAST_MS);
  }

  dismissToast(): void {
    if (this.#timer !== null) clearTimeout(this.#timer);
    this.#timer = null;
    this.toast = null;
  }

  async clear(): Promise<void> {
    await this.#client.call("live-feed.clear", {});
    this.items = [];
  }
}

export const feed = new FeedStore();

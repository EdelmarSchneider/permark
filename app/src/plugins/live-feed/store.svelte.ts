// The live feed: every file the project's disk reported as created, changed, removed or renamed, newest first, and
// the quiet toast that says documents changed while the person reads something else. Beside it, the person's own
// actions from the project's activity journal (and the hand-off marks) for the panel's mine | outside | all filter.
// Dormant until the project calls for it: nothing is drawn before the first change from outside PerMark (then for
// good, remembered per project); while the panel is not on screen, the new outside rows are counted.

import { api, type ActivityEntry, type Api, type FeedItem } from "../../api";
import { documents } from "../../core/doc/document.svelte";
import { changedSections } from "../../core/doc/outline";
import { InFront } from "../../core/inFront.svelte";
import { PluginClient, pluginClient, workspaceRoot } from "../../core/plugins/client";
import { layout } from "../../core/shell/layout.svelte";
import { activeRight } from "../../core/shell/panels.svelte";
import { ui } from "../../core/shell/ui.svelte";
import { silence } from "../../core/silence/silence.svelte";
import { panelRows, pathOf, type FeedShow, type PanelRow } from "./mine";

/** Items kept (the backend keeps as many). */
const MAX_ITEMS = 500;
/** How long a toast stays. */
export const TOAST_MS = 6000;
/** A change this recent means the agent is working. */
export const ACTIVE_MS = 30_000;
/** How long a block counts as changed for the outline (= the glow). */
export const CHANGED_MS = 3000;
/** The id of the Changes panel. */
export const PANEL = "live-feed.panel";
/** The project session key that remembers the project called for the feed once (`true`; absent: dormant). */
export const OFFERED_KEY = "feed.offered";

/** The project session, as far as the feed uses it. */
export type SessionPort = Pick<Api, "projectSessionGet" | "projectSessionSet">;

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

/** A change from outside PerMark (an agent, a sync tool, a colleague): what wakes the dormant feed up. */
export function isOutsideChange(item: FeedItem, silenced: (path: string) => boolean): boolean {
  switch (item.item) {
    case "entry":
      return !item.own && !silenced(item.path);
    case "group":
      return !item.own;
    case "resync":
    case "project":
      return false;
    default:
      return item satisfies never;
  }
}

/** The count as the chip and the tab show it. */
export function countText(n: number): string {
  return n > 99 ? "99+" : String(n);
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
  /** The project called for the feed once (a change from outside, or the person asked): the chip + the tab show. */
  offered = $state(false);
  /** New outside rows while the panel was not on screen (this window, since it opened). */
  unseen = $state(0);
  /** Blocks of the shown document that glow now (the outline marks their sections). */
  glowing = $state.raw<{ path: string; ids: ReadonlySet<string> } | null>(null);
  readonly #client: PluginClient;
  readonly #root: () => string | null;
  readonly #shown: () => string | null;
  readonly #session: SessionPort;
  readonly #onScreen: () => boolean;
  readonly #front = new InFront();
  #timer: ReturnType<typeof setTimeout> | null = null;
  #held = false;
  /** The project the feed follows now. */
  #project: string | null = null;
  readonly #glowTimers = new Set<ReturnType<typeof setTimeout>>();

  constructor(
    client: PluginClient = pluginClient,
    root: () => string | null = workspaceRoot,
    shown: () => string | null = () => documents.path,
    session: SessionPort = api,
    onScreen?: () => boolean,
  ) {
    this.#client = client;
    this.#root = root;
    this.#shown = shown;
    this.#session = session;
    this.#onScreen =
      onScreen ??
      (() =>
        activeRight() === PANEL && !layout.rightHidden && ui.focus !== "on" && this.#front.value);
  }

  /** The Changes panel is on screen: what comes in is seen. */
  get onScreen(): boolean {
    return this.#onScreen();
  }

  /** The outline items whose sections hold a block that glows now. */
  readonly changed = $derived.by((): ReadonlySet<number> => {
    const glow = this.glowing;
    const view = documents.view;
    if (glow === null || view?.path !== glow.path || view.body.kind !== "markdown") {
      return new Set();
    }
    return changedSections(view.body.outline, view.body.blocks, glow.ids);
  });

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
      $effect(() => this.#front.start());
      // what comes in while the panel is on screen is seen
      $effect(() => {
        if (this.#onScreen()) this.unseen = 0;
      });
      $effect(() => {
        const root = this.#root();
        if (root === null) {
          this.#reset();
          return;
        }
        // another project: its own offer, its own count
        if (this.#project !== null && this.#project !== root) this.#reset();
        this.#project = root;
        let off: (() => void) | null = null;
        let gone = false;
        let offMine: (() => void) | null = null;
        void this.#session.projectSessionGet(OFFERED_KEY).then((result) => {
          if (!gone && result.ok && result.value === true) this.offered = true;
        });
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
    if (items.some((i) => isOutsideChange(i, (path) => silence.isSilenced(path)))) this.offer();
  }

  #reset(): void {
    this.#project = null;
    this.items = [];
    this.mine = [];
    this.lastAt = 0;
    this.offered = false;
    this.unseen = 0;
    this.#clearGlow();
    this.dismissToast();
  }

  /** The project calls for the feed (a change from outside, the person asked): shown from now on, remembered. */
  offer(): void {
    if (this.offered) return;
    this.offered = true;
    if (this.#root() !== null) void this.#session.projectSessionSet(OFFERED_KEY, true);
  }

  /** Items the backend reported: listed, and the document ones that changed behind the person's back say so. */
  take(upsert: readonly FeedItem[]): void {
    const known = new Set(this.items.map((i) => i.id));
    this.items = mergeItems(this.items, upsert);
    const heard = upsert.filter((i) => i.item !== "entry" || !silence.isSilenced(i.path));
    this.lastAt = Math.max(this.lastAt, ...heard.map((i) => i.atMs));
    if (heard.some((i) => isOutsideChange(i, () => false))) this.offer();
    if (!this.#onScreen()) {
      // one per new row the panel lists under Outside with the current filters
      const fresh = heard.filter(
        (i) =>
          !known.has(i.id) &&
          !((i.item === "entry" || i.item === "group") && i.own) &&
          (!this.docsOnly || isDocItem(i)),
      );
      this.unseen += new Set(fresh.map((i) => i.id)).size;
    }
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
    this.#timer = null;
    // the pointer is over it | the focus inside: it stays until they leave
    if (this.#held) return;
    this.#timer = setTimeout(() => this.dismissToast(), TOAST_MS);
  }

  /** The pointer came over the toast | the focus went inside: it stays (WCAG 2.2.1). */
  holdToast(): void {
    this.#held = true;
    if (this.#timer !== null) clearTimeout(this.#timer);
    this.#timer = null;
  }

  /** The pointer | focus left: the toast gets its full time again. */
  releaseToast(): void {
    this.#held = false;
    if (this.toast !== null) this.#arm();
  }

  dismissToast(): void {
    if (this.#timer !== null) clearTimeout(this.#timer);
    this.#timer = null;
    this.#held = false;
    this.toast = null;
  }

  /** Blocks of the shown document started to glow: their sections are marked for as long. */
  glow(path: string, ids: readonly string[]): void {
    if (ids.length === 0) return;
    const before = this.glowing?.path === path ? this.glowing.ids : new Set<string>();
    this.glowing = { path, ids: new Set([...before, ...ids]) };
    const timer = setTimeout(() => {
      this.#glowTimers.delete(timer);
      const now = this.glowing;
      if (now?.path !== path) return;
      const left = [...now.ids].filter((id) => !ids.includes(id));
      this.glowing = left.length === 0 ? null : { path, ids: new Set(left) };
    }, CHANGED_MS);
    this.#glowTimers.add(timer);
  }

  #clearGlow(): void {
    for (const timer of this.#glowTimers) clearTimeout(timer);
    this.#glowTimers.clear();
    this.glowing = null;
  }

  /** The person asks for the Changes panel: it shows | closes (and the feed is offered from now on). */
  toggle(): void {
    this.offer();
    ui.toggleRight(PANEL);
  }

  async clear(): Promise<void> {
    await this.#client.call("live-feed.clear", {});
    this.items = [];
  }
}

export const feed = new FeedStore();

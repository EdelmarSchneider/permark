// Follow mode: when a document changes on disk behind the person's back, open it. Paused while the person edits.
// The reload itself never loses the reader's place (core); this only decides when to jump.

import type { FeedItem } from "../../api";
import { documents } from "../../core/doc/document.svelte";
import { editing } from "../../core/editing/session.svelte";
import { PluginClient, pluginClient, workspaceRoot } from "../../core/plugins/client";
import { silence } from "../../core/silence/silence.svelte";
import { isDocument } from "../live-feed/store.svelte";

type Entry = Extract<FeedItem, { item: "entry" }>;

/** The document a batch of feed items says to follow: the newest created, changed or moved one that is not silenced. */
export function followTarget(
  upsert: readonly FeedItem[],
  silenced: (path: string) => boolean = (path) => silence.isSilenced(path),
): string | null {
  let target: Entry | null = null;
  for (const item of upsert) {
    if (item.item !== "entry" || item.own || item.isDir || item.kind === "removed") continue;
    if (!isDocument(item.path) || silenced(item.path)) continue;
    if (target === null || item.atMs >= target.atMs) target = item;
  }
  return target?.path ?? null;
}

export class FollowStore {
  /** Follow mode is on. */
  on = $state(false);
  /** The document the window was showing before the last jump (the way back). */
  jumpedFrom = $state<string | null>(null);
  readonly #client: PluginClient;
  readonly #root: () => string | null;
  readonly #busy: () => boolean;

  constructor(
    client: PluginClient = pluginClient,
    root: () => string | null = workspaceRoot,
    busy: () => boolean = () => editing.mode !== "view" || editing.unsaved,
  ) {
    this.#client = client;
    this.#root = root;
    this.#busy = busy;
  }

  start(): () => void {
    return $effect.root(() => {
      $effect(() => {
        if (this.#root() === null) return;
        let off: (() => void) | null = null;
        let gone = false;
        void this.#client
          .on("live-feed:items", (data) => this.consider(data.upsert))
          .then((stop) => {
            if (gone) stop();
            else off = stop;
          });
        return () => {
          gone = true;
          off?.();
        };
      });
      return () => this.stop();
    });
  }

  toggle(): void {
    if (this.on) this.stop();
    else this.on = true;
  }

  stop(): void {
    this.on = false;
    this.jumpedFrom = null;
  }

  /** Items the feed reported: jumps to the document being written, unless the person is working. */
  consider(upsert: readonly FeedItem[]): void {
    if (!this.on || this.#busy()) return;
    const target = followTarget(upsert);
    if (target === null || target === documents.path) return;
    const here = documents.path;
    // the first jump of a run remembers where the person was; later jumps keep it
    if (this.jumpedFrom === null && here !== null) this.jumpedFrom = here;
    documents.navigate(target, null);
  }

  /** Back to the document the window showed before following took it away. */
  goBack(): void {
    const to = this.jumpedFrom;
    this.jumpedFrom = null;
    if (to !== null) documents.navigate(to, null);
  }
}

export const follow = new FollowStore();

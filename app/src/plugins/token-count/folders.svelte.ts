// The folder count as this window sees it: the job it asked for (progress, the guard's questions), the counted folders
// of the project (their badges), the summary the dialog shows, and the totals counted by themselves as the person opens
// folders. The counting is the backend's; this keeps what the window shows and carries the person's answers.

import {
  api as defaultApi,
  type Api,
  type FolderBadge,
  type FolderJob,
  type FolderSummary,
  type PluginMethods,
  type Result,
} from "../../api";
import { apiErrorText } from "../../core/errors";
import { language, t, tn } from "../../core/i18n";
import { pluginClient, type PluginClient } from "../../core/plugins/client";
import type { TreeBadge } from "../../core/plugins/registry.svelte";
import { windowStore } from "../../core/window.svelte";
import { badgeText, failedKey, folderName, summaryText, type Say } from "./folders";
import { tokens, type TokenStore } from "./store.svelte";
import { infoOf } from "./tokenizers";

const IDLE: FolderJob = { state: "idle" };

/** The calls that answer with the window's job. */
type JobMethod =
  | "token-count.folderStart"
  | "token-count.folderConfirm"
  | "token-count.folderCancel"
  | "token-count.folderDismiss"
  | "token-count.folderJob";
/** Folders waiting to be counted by themselves. */
const QUEUE_MAX = 20;

export class FolderStore {
  /** What the window's job is doing. */
  job = $state.raw<FolderJob>(IDLE);
  /** The summary the dialog shows (`null`: no dialog). */
  summary = $state.raw<FolderSummary | null>(null);
  #badges = $state.raw<ReadonlyMap<string, FolderBadge>>(new Map());
  /** The project whose badges these are. */
  #root: string | null = null;
  /** The folder the person asked for: its summary opens when the job ends. */
  #asked: string | null = null;
  #queue: string[] = [];
  /** Events of the job heard so far: an answer older than the newest event is not taken. */
  #heard = 0;
  /** Folders open in the Explorer when last looked at (`null`: not looking). */
  #seen: ReadonlySet<string> | null = null;
  readonly #api: Api;
  readonly #client: PluginClient;
  readonly #tokens: TokenStore;

  constructor(
    api: Api = defaultApi,
    client: PluginClient = pluginClient,
    tokenStore: TokenStore = tokens,
  ) {
    this.#api = api;
    this.#client = client;
    this.#tokens = tokenStore;
  }

  /** Follows the backend; returns the teardown. */
  start(): () => void {
    let gone = false;
    const offs: (() => void)[] = [];
    const keep = (listening: Promise<() => void>): void => {
      listening.then(
        (off) => {
          if (gone) off();
          else offs.push(off);
        },
        () => {
          // no backend to listen to (a test, a browser)
        },
      );
    };
    keep(
      this.#api.onAppPlugin("token-count:folder", (event) => {
        if (event.window === windowStore.label) this.#hear(event.job);
      }),
    );
    keep(
      this.#api.onAppPlugin("token-count:folders", (event) => {
        if (event.root === this.#root) this.#setBadges(event.folders);
      }),
    );
    return () => {
      gone = true;
      for (const off of offs) off();
      this.#root = null;
      this.#badges = new Map();
      this.#asked = null;
      this.#queue = [];
      this.#seen = null;
      this.job = IDLE;
      this.summary = null;
    };
  }

  /** The window shows another project (or none): its counted folders, and what the job is doing. */
  async load(root: string | null): Promise<void> {
    if (root === this.#root) return;
    this.#root = root;
    this.#badges = new Map();
    this.#queue = [];
    this.#seen = null;
    this.summary = null;
    if (root === null) return;
    const [badges] = await Promise.all([
      this.#client.call("token-count.folders", { root }),
      this.#ask("token-count.folderJob", {}),
    ]);
    if (root !== this.#root) return;
    if (badges.ok) this.#setBadges(badges.value.folders);
  }

  /**
   * Asks the backend about the job and takes its answer, unless an event arrived while it was on its way: the event is
   * newer (a thread that works fast announces the next state before the call that started it is answered).
   */
  async #ask<M extends JobMethod>(
    method: M,
    args: PluginMethods[M]["args"],
  ): Promise<Result<PluginMethods[M]["result"]>> {
    const before = this.#heard;
    const result = await this.#client.call(method, args);
    if (result.ok && this.#heard === before) this.job = result.value;
    return result;
  }

  #setBadges(folders: readonly FolderBadge[]): void {
    this.#badges = new Map(folders.map((badge) => [badge.folder, badge]));
  }

  // ---- what the person sees --------------------------------------------------------------------------------------

  /** The progress of a job that works: `null` when none does. */
  get progress(): { done: number; total: number } | null {
    const job = this.job;
    if (job.state === "counting") return { done: job.done, total: job.total };
    if (job.state === "scanning") return { done: 0, total: 0 };
    return null;
  }

  /** The folder has a result (counted, maybe stale). */
  has(folder: string): boolean {
    return this.#badges.has(folder);
  }

  /** The counted folders of this project, by path. */
  get counted(): FolderBadge[] {
    return Array.from(this.#badges.values());
  }

  /** Something under the folder changed since it was counted. */
  isStale(folder: string): boolean {
    return this.#badges.get(folder)?.stale ?? false;
  }

  /** The badge of a folder row in the Explorer. */
  badge(path: string, isDir: boolean): TreeBadge | null {
    if (!isDir) return null;
    const badge = this.#badges.get(path);
    if (badge === undefined) return null;
    const info = infoOf(badge.tokenizer);
    const tip = `${tn("tokens.count", badge.tokens, {
      n: new Intl.NumberFormat(language.locale).format(badge.tokens),
    })} · ${info?.short ?? badge.tokenizer}`;
    return {
      text: badgeText(badge.tokens, badge.approx, badge.stale, language.locale),
      tone: badge.stale ? "modified" : "muted",
      tip: badge.stale ? `${tip}\n${t("tokens.folder.badge.stale")}` : tip,
      ...(badge.stale ? { title: t("tokens.folder.badge.stale") } : {}),
    };
  }

  // ---- the person's actions --------------------------------------------------------------------------------------

  /** Counts a folder (`""`: the project): the summary opens when it ends. */
  async count(folder: string): Promise<void> {
    const primary = this.#tokens.primary;
    if (primary === null) {
      this.#tokens.openChooser();
      return;
    }
    if (this.job.state !== "idle") {
      windowStore.notifyNote(t("tokens.folder.busy"));
      return;
    }
    this.#asked = folder;
    const result = await this.#ask("token-count.folderStart", { folder, tokenizer: primary.id });
    if (!result.ok) {
      this.#asked = null;
      windowStore.notifyNote(apiErrorText(result.error));
    }
  }

  /** The person agreed to the big folder. */
  async confirm(): Promise<void> {
    await this.#ask("token-count.folderConfirm", {});
  }

  /** Stops the job (or says no to the guard's question). */
  async cancel(): Promise<void> {
    this.#asked = null;
    await this.#ask("token-count.folderCancel", {});
  }

  /** The person has read how the job ended. */
  async dismiss(): Promise<void> {
    await this.#ask("token-count.folderDismiss", {});
  }

  async showSummary(folder: string): Promise<void> {
    const root = this.#root;
    if (root === null) return;
    const result = await this.#client.call("token-count.folderSummary", { root, folder });
    if (result.ok && result.value !== null) this.#show(result.value);
  }

  /** The summary opens over the window: the status popover has done its part. */
  #show(summary: FolderSummary): void {
    this.#tokens.popover = false;
    this.#tokens.choosing = false;
    this.summary = summary;
  }

  closeSummary(): void {
    this.summary = null;
  }

  /** Counts the folder of the summary again. */
  async again(): Promise<void> {
    const open = this.summary;
    if (open === null) return;
    this.summary = null;
    await this.count(open.folder);
  }

  /** The summary as plain text on the clipboard. */
  async copy(): Promise<void> {
    const open = this.summary;
    if (open === null) return;
    const number = new Intl.NumberFormat(language.locale);
    const say: Say = { t, tn, number: (n) => number.format(n) };
    try {
      await navigator.clipboard.writeText(summaryText(open, infoOf(open.tokenizer), say));
      windowStore.notifyNote(t("tokens.folder.summary.copied"));
    } catch {
      windowStore.notifyNote(t("tokens.folder.summary.notCopied"));
    }
  }

  /** The name the dialogs call a folder by. */
  nameOf(folder: string): string {
    const number = new Intl.NumberFormat(language.locale);
    return folderName(folder, { t, tn, number: (n) => number.format(n) });
  }

  // ---- what the backend tells ------------------------------------------------------------------------------------

  #hear(job: FolderJob): void {
    this.#heard += 1;
    this.job = job;
    switch (job.state) {
      case "done":
        void this.#finished(job.folder);
        break;
      case "failed":
        this.#asked = null;
        windowStore.notifyNote(t(failedKey(job.reason)));
        void this.dismiss();
        break;
      case "cancelled":
        this.#asked = null;
        void this.dismiss();
        break;
      case "idle":
        this.#pump();
        break;
      case "scanning":
      case "confirm":
      case "refused":
      case "counting":
        break;
      default:
        job satisfies never;
    }
  }

  async #finished(folder: string): Promise<void> {
    const shown = this.#asked === folder;
    if (shown) this.#asked = null;
    const root = this.#root;
    let summary: FolderSummary | null = null;
    if (shown && root !== null) {
      const result = await this.#client.call("token-count.folderSummary", { root, folder });
      if (result.ok) summary = result.value;
    }
    await this.dismiss();
    if (summary !== null) this.#show(summary);
  }

  // ---- totals counted by themselves ------------------------------------------------------------------------------

  /** The folders open in the Explorer now: the ones that just opened are counted, when they are small. */
  folderOpened(open: ReadonlySet<string>): void {
    const before = this.#seen ?? new Set<string>();
    this.#seen = open;
    for (const folder of open) {
      if (folder === "" || before.has(folder)) continue;
      const known = this.#badges.get(folder);
      if (known !== undefined && !known.stale) continue;
      if (!this.#queue.includes(folder) && this.#queue.length < QUEUE_MAX) {
        this.#queue.push(folder);
      }
    }
    this.#pump();
  }

  /** The totals are not wanted (any more): nothing waits. */
  stopAuto(): void {
    this.#queue = [];
    this.#seen = null;
  }

  #pump(): void {
    if (this.job.state !== "idle" || !(this.#tokens.state?.folderTotals ?? false)) return;
    const folder = this.#queue.shift();
    const primary = this.#tokens.primary;
    if (folder === undefined || primary === null) return;
    void this.#ask("token-count.folderStart", { folder, tokenizer: primary.id, auto: true });
  }
}

export const folders = new FolderStore();

// The repositories of the included projects (`projects.md` §8, §12): their summaries (`git.repos` + `git:repos`), the
// changed files of the ones a view looks at (`git:projectSnapshot`), and the interest this view declares
// (`git.interest`): what the window shows (`projects.looking`) + the project of the active document (status bar).
// Interest is sent a moment after it settles, only when it changed; `[]` when the view hides or goes (the backend has
// no other way to know).

import { untrack } from "svelte";
import type { GitFile, GitSnapshot, InterestLevel, RepoSummary } from "../../api";
import { PluginClient, pluginClient, workspaceRoot } from "../../core/plugins/client";
import { documents } from "../../core/doc/document.svelte";
import { projects } from "../../core/projects/instance.svelte";
import { keyOf } from "../../core/projects/model";
import type { Looking } from "../../core/projects/store.svelte";
import { tabs } from "../../core/shell/tabs.svelte";
import { windowStore } from "../../core/window.svelte";
import { silence } from "../../core/silence/silence.svelte";
import { repoOf, reposByKey } from "./facts";

/** Interest settles this long before it is sent (scrolling the tree, opening folders). */
export const INTEREST_MS = 150;
/** The backend takes at most this many ids per set. */
export const INTEREST_MAX = 500;

/** What the members store needs from its window. */
export type MembersDeps = {
  client: PluginClient;
  root: () => string | null;
  /** The projects the window shows, by level. */
  looking: () => Looking;
  /** The active document's path (root-relative; `null`: none). */
  document: () => string | null;
  /** This view's id for `git.interest` (one set per view). */
  view: () => string;
  /** The view is on screen (a tab behind another one is not). */
  shown: () => boolean;
};

type Sent = { view: string; glance: readonly string[]; detail: readonly string[] };

const LEVELS: readonly InterestLevel[] = ["glance", "detail"];

function same(a: readonly string[], b: readonly string[]): boolean {
  return a.length === b.length && a.every((x, n) => x === b[n]);
}

export class GitMembers {
  /** Summaries by project uid. */
  repos = $state.raw<ReadonlyMap<string, RepoSummary>>(new Map());
  /** Changed files of the repositories a view looks at, by project uid. */
  lists = $state.raw<ReadonlyMap<string, GitSnapshot>>(new Map());
  byKey = $derived(reposByKey(this.repos.values()));
  /** Files of the kept lists by path. */
  #files = $derived.by(() => {
    const files = new Map<string, { file: GitFile; list: GitSnapshot }>();
    for (const list of this.lists.values()) {
      for (const file of list.files) files.set(file.path, { file, list });
    }
    return files;
  });
  /** Folders inside a project (the project folder included): how many changed files are below. */
  #folders = $derived.by(() => {
    const folders = new Map<string, number>();
    for (const [uid, list] of this.lists) {
      const repo = this.repos.get(uid);
      if (!repo) continue;
      for (const file of list.files) {
        if (silence.isSilenced(file.path)) continue;
        const parts = file.path.split("/");
        for (let at = 1; at < parts.length; at++) {
          const folder = parts.slice(0, at).join("/");
          if (folder.length < repo.path.length) continue;
          folders.set(folder, (folders.get(folder) ?? 0) + 1);
        }
      }
    }
    return folders;
  });

  readonly #deps: MembersDeps;
  #sent: Sent | null = null;
  #timer: ReturnType<typeof setTimeout> | null = null;
  #asked = new Set<string>();
  #generation = 0;

  constructor(deps: MembersDeps) {
    this.#deps = deps;
  }

  /** The repository of the active document (`null`: the opened folder's own | none). */
  get active(): RepoSummary | null {
    return repoOf(this.byKey, this.#deps.document());
  }

  /** Follows the repositories of the window's folder until the returned teardown runs. */
  start(): () => void {
    return $effect.root(() => {
      $effect(() => {
        const root = this.#deps.root();
        if (root === null) {
          untrack(() => this.#clear());
          return;
        }
        const generation = ++this.#generation;
        const offs: (() => void)[] = [];
        let gone = false;
        const keep = (stop: () => void): void => {
          if (gone) stop();
          else offs.push(stop);
        };
        void this.#deps.client
          .on("git:repos", (data) => this.#apply(data.upsert, data.removed))
          .then(keep);
        void this.#deps.client
          .on("git:projectSnapshot", (data) => this.#list(data.project, data.snapshot))
          .then(keep);
        void this.#deps.client.call("git.repos", {}).then((result) => {
          if (generation === this.#generation && result.ok) this.#apply(result.value, [], true);
        });
        return () => {
          gone = true;
          for (const off of offs) off();
          untrack(() => this.#clear());
        };
      });
      // what this view looks at, sent once it settles
      $effect(() => {
        const wanted = this.wanted();
        const view = this.#deps.view();
        untrack(() => this.#schedule(view, wanted));
      });
      return () => this.#clear();
    });
  }

  /** The ids this view wants now, by level (known repositories only; none while hidden). */
  wanted(): { glance: string[]; detail: string[] } {
    if (!this.#deps.shown() || this.#deps.root() === null) return { glance: [], detail: [] };
    const looking = this.#deps.looking();
    const known = (ids: Iterable<string>): string[] =>
      [...new Set(ids)]
        .filter((id) => this.repos.has(id))
        .sort()
        .slice(0, INTEREST_MAX);
    const active = this.active;
    return {
      glance: known(active === null ? looking.glance : [...looking.glance, active.id]),
      detail: known(looking.detail),
    };
  }

  #schedule(view: string, wanted: { glance: string[]; detail: string[] }): void {
    if (this.#timer !== null) clearTimeout(this.#timer);
    this.#timer = setTimeout(() => {
      this.#timer = null;
      this.#send({ view, ...wanted });
    }, INTEREST_MS);
  }

  #send(next: Sent): void {
    const before = this.#sent;
    if (before !== null && before.view !== next.view) {
      // another view id (the Welcome view became a tab): the old sets go first
      for (const level of LEVELS) {
        if (before[level].length > 0) this.#call([], level, before.view);
      }
      this.#sent = null;
    }
    const was = this.#sent ?? { view: next.view, glance: [], detail: [] };
    for (const level of LEVELS) {
      if (!same(was[level], next[level])) this.#call(next[level], level, next.view);
    }
    this.#sent = next;
    // lists of projects nobody looks at any more are dropped (counts stay in the summaries)
    const kept = new Set([...next.glance, ...next.detail]);
    for (const id of this.#asked) if (!kept.has(id)) this.#asked.delete(id);
    if ([...this.lists.keys()].some((id) => !kept.has(id))) {
      this.lists = new Map([...this.lists].filter(([id]) => kept.has(id)));
    }
    for (const id of kept) this.#askList(id);
  }

  #call(ids: readonly string[], level: InterestLevel, view: string): void {
    void this.#deps.client.call("git.interest", { ids: [...ids], level, view });
  }

  /** A list not known yet for a project the view looks at: asked once its first count is there (the backend has it
   *  then; before, the event brings it). */
  #askList(id: string): void {
    if (this.lists.has(id) || this.#asked.has(id)) return;
    const repo = this.repos.get(id);
    if (!repo || repo.changed === null || repo.error !== null) return;
    this.#asked.add(id);
    const generation = this.#generation;
    void this.#deps.client.call("git.snapshot", { project: id }).then((result) => {
      if (generation !== this.#generation || !result.ok || result.value === null) return;
      this.#list(id, result.value);
    });
  }

  #wants(id: string): boolean {
    const sent = this.#sent;
    return sent !== null && (sent.glance.includes(id) || sent.detail.includes(id));
  }

  #apply(upsert: readonly RepoSummary[], removed: readonly string[], whole = false): void {
    const next = new Map(whole ? [] : this.repos);
    for (const id of removed) next.delete(id);
    for (const s of upsert) next.set(s.id, s);
    this.repos = next;
    if (removed.some((id) => this.lists.has(id))) {
      this.lists = new Map([...this.lists].filter(([id]) => next.has(id)));
    }
    for (const s of upsert) if (this.#wants(s.id)) this.#askList(s.id);
  }

  #list(id: string, snapshot: GitSnapshot | null): void {
    const known = this.lists.get(id);
    if (snapshot === null) {
      if (known) this.lists = new Map([...this.lists].filter(([key]) => key !== id));
      return;
    }
    // a late answer for a project nobody looks at | an older picture is dropped
    if (!this.#wants(id) || (known && snapshot.seq < known.seq)) return;
    const next = new Map(this.lists);
    next.set(id, snapshot);
    this.lists = next;
  }

  #clear(): void {
    this.#generation += 1;
    if (this.#timer !== null) clearTimeout(this.#timer);
    this.#timer = null;
    const sent = this.#sent;
    if (sent !== null) {
      for (const level of LEVELS) if (sent[level].length > 0) this.#call([], level, sent.view);
    }
    this.#sent = null;
    this.#asked.clear();
    if (this.repos.size > 0) this.repos = new Map();
    if (this.lists.size > 0) this.lists = new Map();
  }

  // ---- lookups ---------------------------------------------------------------------------------------------------

  /** The summary of a project (`null`: no repository followed for it). */
  of(uid: string | null): RepoSummary | null {
    return uid === null ? null : (this.repos.get(uid) ?? null);
  }

  /** The repository of an included project holding `path` (`null`: the opened folder's). */
  ownerOf(path: string): RepoSummary | null {
    return repoOf(this.byKey, path);
  }

  /** A changed file inside a project, with the list it came from (`undefined`: not in a kept list). */
  fileOf(path: string): { file: GitFile; list: GitSnapshot } | undefined {
    return this.#files.get(path);
  }

  /** How many changed files are below a folder inside a project; the project folder itself counts from its summary
   *  until its list is kept (silenced files never count). */
  changedBelow(repo: RepoSummary, folder: string): number {
    if (keyOf(folder) === keyOf(repo.path) && !this.lists.has(repo.id)) {
      return silence.isSilenced(folder) ? 0 : (repo.changed ?? 0);
    }
    return this.#folders.get(folder) ?? 0;
  }
}

/** This window's members store. */
export const members = new GitMembers({
  client: pluginClient,
  root: workspaceRoot,
  looking: () => projects.looking,
  // the page's path (reactive; `path` only moves with the controller)
  document: () => documents.shown,
  view: () => String(windowStore.tab ?? "window"),
  // a tab behind another one is not on screen
  shown: () => tabs.active === null || windowStore.tab === null || tabs.active === windowStore.tab,
});

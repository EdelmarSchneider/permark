// The git plugin's picture of the project: the snapshot the backend keeps (HEAD, branch, state, changed files with
// their letters) and what the rows of the Explorer and the header ask of it. The backend tells when it changes.

import type { BaselineKind, GitFile, GitSnapshot, Letter } from "../../api";
import { PluginClient, pluginClient, workspaceRoot } from "../../core/plugins/client";
import type { BadgeTone, TreeBadge } from "../../core/plugins/registry.svelte";
import { t, tn } from "../../core/i18n";
import { silence } from "../../core/silence/silence.svelte";

export type GitState =
  | { status: "off" }
  | { status: "loading" }
  | { status: "none" }
  | { status: "ready"; snapshot: GitSnapshot };

const LETTERS: Readonly<Record<Letter, { letter: string; tone: BadgeTone }>> = {
  modified: { letter: "M", tone: "modified" },
  added: { letter: "A", tone: "added" },
  untracked: { letter: "U", tone: "untracked" },
  deleted: { letter: "D", tone: "deleted" },
  renamed: { letter: "R", tone: "renamed" },
  conflict: { letter: "C", tone: "conflict" },
};

export const LETTER_ORDER: readonly Letter[] = [
  "modified",
  "added",
  "untracked",
  "deleted",
  "renamed",
  "conflict",
];

export function letterOf(letter: Letter): string {
  return LETTERS[letter].letter;
}

/** The folders above a path (`a/b/c.md` -> `a`, `a/b`). */
function ancestors(path: string): string[] {
  const parts = path.split("/");
  const out: string[] = [];
  for (let at = 1; at < parts.length; at++) out.push(parts.slice(0, at).join("/"));
  return out;
}

export class GitStore {
  state = $state.raw<GitState>({ status: "off" });
  /** The legend popover is open. */
  legend = $state(false);
  #files = $state.raw<ReadonlyMap<string, GitFile>>(new Map());
  /** Folders: how many changed files are below (what the person silenced does not count). */
  #folders = $derived.by(() => {
    const folders = new Map<string, number>();
    for (const file of this.#files.values()) {
      if (silence.isSilenced(file.path)) continue;
      for (const folder of ancestors(file.path))
        folders.set(folder, (folders.get(folder) ?? 0) + 1);
    }
    return folders;
  });
  #seq = 0;
  readonly #client: PluginClient;
  readonly #root: () => string | null;

  constructor(client: PluginClient = pluginClient, root: () => string | null = workspaceRoot) {
    this.#client = client;
    this.#root = root;
  }

  get snapshot(): GitSnapshot | null {
    return this.state.status === "ready" ? this.state.snapshot : null;
  }

  /** Follows the project of the window until the returned teardown runs. */
  start(): () => void {
    return $effect.root(() => {
      $effect(() => {
        if (this.#root() === null) {
          this.#clear();
          return;
        }
        let off: (() => void) | null = null;
        let gone = false;
        void this.#client
          .on("git:snapshot", (data) => this.#apply(data))
          .then((stop) => {
            if (gone) stop();
            else off = stop;
          });
        void this.#load();
        return () => {
          gone = true;
          off?.();
        };
      });
      // a window that gets the focus again looks at the repository again
      $effect(() => {
        const onFocus = (): void => void this.refresh();
        window.addEventListener("focus", onFocus);
        return () => window.removeEventListener("focus", onFocus);
      });
      return () => this.#clear();
    });
  }

  async #load(): Promise<void> {
    this.state = { status: "loading" };
    const result = await this.#client.call("git.snapshot", {});
    if (result.ok) this.#apply(result.value);
    else this.state = { status: "none" };
  }

  #apply(snapshot: GitSnapshot | null): void {
    if (snapshot === null) {
      this.#files = new Map();
      this.state = { status: "none" };
      return;
    }
    // a late announcement of an older picture is ignored
    if (this.state.status === "ready" && snapshot.seq < this.state.snapshot.seq) return;
    const files = new Map<string, GitFile>();
    for (const file of snapshot.files) files.set(file.path, file);
    this.#files = files;
    this.state = { status: "ready", snapshot };
    this.#seq = snapshot.seq;
  }

  #clear(): void {
    this.#files = new Map();
    this.state = { status: "off" };
  }

  /** Asks the backend to look again (the window got the focus). */
  async refresh(): Promise<void> {
    if (this.state.status === "off") return;
    await this.#client.call("git.refresh", {});
  }

  async setBaseline(kind: BaselineKind): Promise<void> {
    await this.#client.call("git.setBaseline", { kind });
  }

  fileOf(path: string): GitFile | undefined {
    return this.#files.get(path);
  }

  /** How many changed files are below a folder. */
  changedBelow(folder: string): number {
    return this.#folders.get(folder) ?? 0;
  }

  /** What the row of a path says: a letter on a file, a dot with the count on a folder. */
  badge(path: string, isDir: boolean): TreeBadge | null {
    if (this.state.status !== "ready" || silence.isSilenced(path)) return null;
    if (isDir) {
      const n = this.#folders.get(path) ?? 0;
      return n === 0 ? null : { text: "•", tone: "modified", title: tn("git.folderChanged", n) };
    }
    const file = this.#files.get(path);
    if (!file) return null;
    const { letter, tone } = LETTERS[file.letter];
    return { text: letter, tone, title: this.explain(file) };
  }

  /** The letter in plain words. */
  explain(file: GitFile): string {
    const baseline = this.snapshot?.baseline.kind ?? "head";
    const since = t(baseline === "lastVisit" ? "git.since.visit" : "git.since.commit");
    const text = t(`git.explain.${file.letter}`, { since });
    return file.letter === "renamed" && file.from ? `${text} (${file.from})` : text;
  }

  /** Files changed (what the status bar counts). */
  get changed(): number {
    return this.snapshot?.files.length ?? 0;
  }

  /** The last announcement the store applied (tests). */
  get seq(): number {
    return this.#seq;
  }
}

export const git = new GitStore();

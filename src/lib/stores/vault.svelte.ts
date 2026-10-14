// The open project: vault info, file tree, git snapshot, "since last visit" and search indexing progress.

import { SvelteSet } from "svelte/reactivity";
import { api } from "../api";
import { mapPath, mapSet } from "../fileOps";
import { planReveal } from "../reveal";
import { collectFiles, rollups, splitRoots } from "../tree";
import type { ChangedSince, GitSnapshot, Move, TreeNode, VaultInfo } from "../types";
import { ui } from "./ui.svelte";

export type WorktreeInfo = VaultInfo["worktrees"][number];

class VaultStore {
  info = $state.raw<VaultInfo | null>(null);
  tree = $state.raw<TreeNode | null>(null);
  git = $state.raw<GitSnapshot | null>(null);
  since = $state.raw<ChangedSince | null>(null);
  sinceLoaded = $state(false);
  stateLocation = $state<"appData" | "project">("appData");
  indexProgress = $state.raw<{ indexed: number; total: number } | null>(null);
  expanded = new SvelteSet<string>();
  /** Asks the tree to scroll a row into view. */
  reveal = $state.raw<{ path: string; n: number } | null>(null);

  roots = $derived(this.tree ? splitRoots(this.tree) : { main: [], worktrees: [] });
  rollups = $derived(this.git ? rollups(this.git.files) : new Map<string, never>());
  files = $derived(this.tree ? collectFiles(this.tree.children ?? []) : []);
  isSimple = $derived(this.info?.mode === "simple");
  changedCount = $derived(this.git ? Object.keys(this.git.files).length : 0);

  private revealCount = 0;

  reset(): void {
    this.info = null;
    this.tree = null;
    this.git = null;
    this.since = null;
    this.sinceLoaded = false;
    this.indexProgress = null;
    this.expanded.clear();
    this.reveal = null;
  }

  worktree(name: string): WorktreeInfo | undefined {
    return this.info?.worktrees.find((w) => w.name === name);
  }

  async refreshTree(): Promise<void> {
    try {
      this.tree = await api.listTree();
    } catch {
      // keep the previous tree; the next tree-changed event retries
    }
  }

  async refreshSince(): Promise<void> {
    try {
      this.since = await api.sinceLastVisit();
    } catch {
      this.since = null;
    } finally {
      this.sinceLoaded = true;
    }
  }

  toggle(path: string, expand?: boolean): void {
    const open = expand ?? !this.expanded.has(path);
    if (open) this.expanded.add(path);
    else this.expanded.delete(path);
  }

  collapseAll(): void {
    this.expanded.clear();
  }

  /** Files or folders were moved or renamed: the folders that were open stay open in their new place. */
  followMoves(moves: readonly Move[]): void {
    const open = mapSet(this.expanded, moves);
    this.expanded.clear();
    for (const path of open) this.expanded.add(path);
    const cursor = ui.treeCursor;
    if (cursor) ui.treeCursor = { ...cursor, path: mapPath(cursor.path, moves) };
  }

  /** Opens the folders (and the Explorer section) that hold `path`, then selects its row and scrolls it into view. */
  revealPath(path: string): void {
    const plan = planReveal(path, this.expanded);
    for (const dir of plan.expand) this.expanded.add(dir);
    ui.collapsedSections.delete(plan.section);
    this.reveal = { path: plan.target, n: ++this.revealCount };
  }
}

export const vault = new VaultStore();

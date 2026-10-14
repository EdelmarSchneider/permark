// The Explorer filter in force: the person's choice, else the project's default, else the app's. Derives
// the filtered tree and the lists Quick open needs. "Show all" lifts the filter for now and is not saved.

import { documentFolders, fileTest, filterFromDefault, filterTree } from "../treeFilter";
import type { TreeFilter } from "../types";
import { docs } from "./docs.svelte";
import { session } from "./session.svelte";
import { vault } from "./vault.svelte";

const NO_PATHS: ReadonlySet<string> = new Set();

class FilterStore {
  /** The filter is lifted for now. Any change of the filter ends it; it is not remembered. */
  showAll = $state(false);

  current = $derived<TreeFilter>(
    session.treeFilter ?? filterFromDefault(vault.info?.treeDefault ?? null),
  );

  /** Folders that hold a document, for "only assets next to documents". */
  private docFolders = $derived(documentFolders(vault.files));

  // The open document stays in the tree even when the filter hides it.
  private outcome = $derived(
    filterTree(vault.tree, fileTest(this.current, this.docFolders), {
      hideEmptyFolders: this.current.hideEmptyFolders,
      keep: docs.path,
    }),
  );

  /** What the Explorer lists. */
  roots = $derived(
    this.showAll ? vault.roots : { main: this.outcome.main, worktrees: this.outcome.worktrees },
  );
  /** The files Quick open lists. */
  files = $derived(this.showAll ? vault.files : this.outcome.shown);
  /** The files the filter keeps out of the Explorer and Quick open right now. */
  hiddenFiles = $derived(this.showAll ? [] : this.outcome.hidden);
  /** Hidden files shown anyway because they are open (drawn dimmed). */
  forced = $derived<ReadonlySet<string>>(this.showAll ? NO_PATHS : this.outcome.forced);

  total = $derived(vault.files.length);
  /** Files the filter would hide, whether or not "Show all" is on. */
  hiddenByFilter = $derived(this.outcome.hidden.length);
  shownByFilter = $derived(this.outcome.shown.length);

  set(filter: TreeFilter): void {
    this.showAll = false;
    session.setTreeFilter(filter);
  }

  setShowAll(on: boolean): void {
    this.showAll = on;
  }

  reset(): void {
    this.showAll = false;
  }
}

export const filter = new FilterStore();

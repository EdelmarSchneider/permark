// The Explorer filter in force: the person's choice, else the project's default, else the app's. Derives
// the filtered tree and the lists Quick open needs. "Show all" lifts the filter for now and is not saved.

import {
  documentFolders,
  fileTest,
  filterFromDefault,
  filterTree,
  sameTreeLoad,
  treeLoadOf,
} from "../treeFilter";
import type { TreeFilter } from "../types";
import { onlyUnread } from "../unread";
import { docs } from "./docs.svelte";
import { fileops } from "./fileops.svelte";
import { session } from "./session.svelte";
import { quiet } from "./quiet.svelte";
import { unread } from "./unread.svelte";
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
    filterTree(vault.tree, fileTest(this.current, this.docFolders, vault.imageRefs), {
      hideEmptyFolders: this.current.hideEmptyFolders,
      keep: docs.path,
      keepFolders: fileops.madeFolders,
      showHidden: this.current.showHidden,
    }),
  );

  /** What the filter lets through. */
  private filtered = $derived(
    this.showAll ? vault.roots : { main: this.outcome.main, worktrees: this.outcome.worktrees },
  );
  /** What the Explorer lists: that, or only what is unread (and the open document, so it does not vanish when read). */
  /** Unread documents the person still follows. A silenced one stays out of "only unread". */
  private heardUnread = $derived.by(() => {
    const heard = new Set<string>();
    for (const path of unread.set) if (!quiet.isQuiet(path)) heard.add(path);
    return heard;
  });
  roots = $derived(
    unread.onlyUnread
      ? {
          main: onlyUnread(this.filtered.main, this.heardUnread, docs.path),
          worktrees: onlyUnread(this.filtered.worktrees, this.heardUnread, docs.path),
        }
      : this.filtered,
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
    const before = treeLoadOf(this.current);
    this.showAll = false;
    session.setTreeFilter(filter);
    // ignored files are not in the tree until it is loaded again
    if (!sameTreeLoad(before, treeLoadOf(filter))) void vault.refreshTree();
  }

  setShowAll(on: boolean): void {
    this.showAll = on;
  }

  reset(): void {
    this.showAll = false;
  }
}

export const filter = new FilterStore();

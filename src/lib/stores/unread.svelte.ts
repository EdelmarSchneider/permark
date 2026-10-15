// The unread markers: which documents changed since the person last read them (the backend keeps the books and
// tells us), when the document on show counts as read (`ReadTracker`), and the Explorer's "show only unread".

import { api } from "../api";
import { errorMessage } from "../errors";
import { ReadTracker, realScheduler, type Scheduler } from "../readTracker";
import { folderCounts } from "../unread";
import { toast } from "./toast.svelte";

export class UnreadStore {
  /** The unread documents, sorted, as the backend last said. */
  paths = $state.raw<readonly string[]>([]);
  set = $derived<ReadonlySet<string>>(new Set(this.paths));
  /** Unread documents inside each folder. */
  folders = $derived(folderCounts(this.paths));
  count = $derived(this.paths.length);
  /** The Explorer lists only what is unread. */
  onlyUnread = $state(false);
  /** The Explorer lists only what is unread and nothing is: it says "All caught up." (the open document may stay listed). */
  caughtUp = $derived(this.onlyUnread && this.count === 0);

  private tracker: ReadTracker;
  private request = 0;

  constructor(scheduler: Scheduler = realScheduler) {
    this.tracker = new ReadTracker({
      scheduler,
      markRead: (path) => void this.markRead(path),
      isUnread: (path) => this.set.has(path),
    });
  }

  isUnread(path: string): boolean {
    return this.set.has(path);
  }

  /** How many documents are unread in a folder (0 for a path that is not one). */
  inFolder(path: string): number {
    return this.folders.get(path) ?? 0;
  }

  /** A project was opened (or closed): forget everything. */
  reset(): void {
    this.request++;
    this.paths = [];
    this.onlyUnread = false;
    this.tracker.show(null, false);
  }

  /** Asks the backend which documents are unread. */
  async refresh(): Promise<void> {
    const request = ++this.request;
    try {
      const paths = await api.getUnread();
      // an event that came meanwhile is newer than this answer
      if (request === this.request) this.take(paths);
    } catch {
      // the next `unread-changed` event tells
    }
  }

  /** `unread-changed`: all the unread documents. */
  onChanged(paths: readonly string[]): void {
    this.request++;
    this.take(paths);
  }

  private take(paths: readonly string[]): void {
    this.paths = paths;
    this.tracker.unreadChanged();
  }

  toggleOnlyUnread(): void {
    this.onlyUnread = !this.onlyUnread;
  }

  // ---- what the person reads -------------------------------------------------------------------------

  /** The document on show, or none (null: nothing open, or not Markdown), and whether the reading view shows it. */
  watch(path: string | null, visible: boolean): void {
    this.tracker.show(path, visible);
  }

  /** The window has the focus (and is not hidden). */
  setFocused(focused: boolean): void {
    this.tracker.setFocused(focused);
  }

  /** The person scrolled the document on show. */
  scrolled(): void {
    this.tracker.scrolled();
  }

  /** The file of the document on show changed on disk. */
  contentChanged(path: string): void {
    this.tracker.contentChanged(path);
  }

  // ---- marking ----------------------------------------------------------------------------------------

  /** Tells the backend a document was read. Quiet: it is not something the person asked for. */
  async markRead(path: string): Promise<void> {
    try {
      await api.markRead(path);
    } catch {
      // the next time the rule passes asks again
    }
  }

  /** "Mark as unread": the document stays unread even while it is on show. */
  async markUnread(path: string, onShow: boolean): Promise<void> {
    if (onShow) this.tracker.hold();
    await this.ask(() => api.markUnread(path));
  }

  async markAllRead(): Promise<void> {
    await this.ask(() => api.markAllRead());
  }

  async markFolderRead(path: string): Promise<void> {
    await this.ask(() => api.markFolderRead(path));
  }

  private async ask(run: () => Promise<void>): Promise<void> {
    try {
      await run();
    } catch (error) {
      toast.message(errorMessage(error), "warn");
    }
  }
}

export const unread = new UnreadStore();

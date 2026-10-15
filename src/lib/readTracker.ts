// When the open document counts as read. Pure: the clock is handed in, so the rules are tested without waiting.
//
// A document is read when it is on show in the reading view of a window that has the focus, and either the person
// scrolled it or it stayed on show for a few seconds. A change that arrives while the window has the focus and the
// document was read already is read as well (the person is watching it happen); one that arrives while the window
// has not is not, and the rule has to pass again. What a document held when it was read is the backend's business
// (it hashes the file): this module only decides when to say "read".

/** How long a document has to stay on show to count as read without being scrolled. */
export const READ_AFTER_MS = 3000;

/** The clock of the tracker: runs a function after a pause and hands back the way to cancel it. */
export type Scheduler = {
  after(ms: number, run: () => void): () => void;
};

export const realScheduler: Scheduler = {
  after(ms, run) {
    const id = setTimeout(run, ms);
    return () => clearTimeout(id);
  },
};

export type ReadTrackerHooks = {
  scheduler: Scheduler;
  /** Tells the backend the person read the document (what it holds now). */
  markRead(path: string): void;
  /** Whether the document is unread right now. */
  isUnread(path: string): boolean;
};

export class ReadTracker {
  private path: string | null = null;
  private focused = false;
  private visible = false;
  /** The rule has passed for the content on show. */
  private qualified = false;
  /** The person marked the document unread on purpose: it stays so until they leave it. */
  private held = false;
  private cancel: (() => void) | null = null;

  constructor(private readonly hooks: ReadTrackerHooks) {}

  /**
   * Which document is on show (null: none, or one that is not Markdown) and whether the reading view shows it
   * (not while it is being edited, or when the file is gone). Call it whenever either changes.
   */
  show(path: string | null, visible: boolean): void {
    if (path !== this.path) {
      this.path = path;
      this.qualified = false;
      this.held = false;
      this.stop();
    } else if (!visible && this.visible) {
      // Leaving the reading view (to edit): what comes back is read again from the start.
      this.qualified = false;
    }
    this.visible = visible;
    this.evaluate();
  }

  /** The window gained or lost the focus (or was hidden). */
  setFocused(focused: boolean): void {
    this.focused = focused;
    this.evaluate();
  }

  /** The person scrolled the document. */
  scrolled(): void {
    if (!this.reading()) return;
    this.qualified = true;
    this.evaluate();
  }

  /** The file of the document on show changed on disk. */
  contentChanged(path: string): void {
    if (path !== this.path) return;
    if (!this.focused || !this.visible) {
      // nobody watched it happen
      this.qualified = false;
      this.stop();
      return;
    }
    // The backend may or may not have noticed the change yet: whatever it holds now is what is on show.
    if (this.qualified && !this.held) this.hooks.markRead(path);
  }

  /** The set of unread documents changed: the one on show may have become unread behind the person's back. */
  unreadChanged(): void {
    this.evaluate();
  }

  /** The person marked the document on show as unread: it is not read again until they leave it. */
  hold(): void {
    this.held = true;
    this.stop();
  }

  private reading(): boolean {
    return this.path !== null && this.focused && this.visible && !this.held;
  }

  private stop(): void {
    this.cancel?.();
    this.cancel = null;
  }

  private evaluate(): void {
    const path = this.path;
    if (path === null || !this.reading()) {
      this.stop();
      return;
    }
    if (!this.qualified) {
      this.cancel ??= this.hooks.scheduler.after(READ_AFTER_MS, () => {
        this.cancel = null;
        this.qualified = true;
        this.evaluate();
      });
      return;
    }
    this.stop();
    if (this.hooks.isUnread(path)) this.hooks.markRead(path);
  }
}

// Autosave for notes: when to save, as a state machine that is told what time it is.
//
// A note is saved 750 ms after typing stops, and at least every 3 s while typing goes on. It is also saved at once when
// the window loses the focus, when the person switches notes, when the window closes and before an update restarts the
// app (the callers say so with `flush`). `AutosaveTimer` decides, from instants it is given, when a save is due; `Autosaver`
// puts it on the clock and keeps saves from overlapping. Nothing here reads a clock but the one it is given.

/** A save is due this long after the last edit... */
export const QUIET_MS = 750;
/** ...but no later than this after the first edit that has not been saved. */
export const MAX_WAIT_MS = 3000;

/** The state machine: edits come in with their time, and it says when the next save is due. */
export class AutosaveTimer {
  private first: number | null = null;
  private last: number | null = null;

  constructor(
    private readonly quiet = QUIET_MS,
    private readonly max = MAX_WAIT_MS,
  ) {}

  /** An edit happened at `now`. */
  touch(now: number): void {
    this.first ??= now;
    this.last = now;
  }

  get pending(): boolean {
    return this.first !== null;
  }

  /** The instant the pending edits should be saved, or null when there are none. */
  deadline(): number | null {
    if (this.first === null || this.last === null) return null;
    return Math.min(this.last + this.quiet, this.first + this.max);
  }

  due(now: number): boolean {
    const deadline = this.deadline();
    return deadline !== null && now >= deadline;
  }

  /** The pending edits were saved (or given up). */
  reset(): void {
    this.first = null;
    this.last = null;
  }
}

export type AutosaverOptions = {
  /** Saves the note. Resolves when it is done; a failure is the caller's to show (it must not throw). */
  save: () => Promise<unknown>;
  /** The time in milliseconds. */
  now?: () => number;
  setTimer?: (run: () => void, ms: number) => unknown;
  clearTimer?: (handle: unknown) => void;
};

/** Saves when the timer says so, one save at a time, and saves at once when asked. */
export class Autosaver {
  private readonly timer = new AutosaveTimer();
  private handle: unknown = null;
  private running: Promise<void> | null = null;
  /** Edits came in while a save was running: they are saved next. */
  private again = false;
  private readonly save: () => Promise<unknown>;
  private readonly now: () => number;
  private readonly setTimer: (run: () => void, ms: number) => unknown;
  private readonly clearTimer: (handle: unknown) => void;

  constructor(options: AutosaverOptions) {
    this.save = options.save;
    this.now = options.now ?? (() => Date.now());
    this.setTimer = options.setTimer ?? ((run, ms) => setTimeout(run, ms));
    this.clearTimer = options.clearTimer ?? ((handle) => clearTimeout(handle as never));
  }

  /** Something is waiting to be saved, or a save is running. */
  get busy(): boolean {
    return this.timer.pending || this.again || this.running !== null;
  }

  /** The note was edited. */
  touch(): void {
    if (this.running) {
      this.again = true;
      return;
    }
    this.timer.touch(this.now());
    this.arm();
  }

  /** Forgets what is waiting (the note is gone, or was saved some other way). */
  cancel(): void {
    this.disarm();
    this.timer.reset();
    this.again = false;
  }

  /**
   * Saves what is waiting now and waits until nothing is: a save that is running finishes first, and edits that came in
   * meanwhile are saved too.
   */
  async flush(): Promise<void> {
    for (let round = 0; round < 4; round++) {
      if (this.running) await this.running;
      if (!this.timer.pending && !this.again) return;
      this.disarm();
      this.timer.reset();
      this.again = false;
      await this.start();
    }
  }

  private arm(): void {
    this.disarm();
    const deadline = this.timer.deadline();
    if (deadline === null) return;
    this.handle = this.setTimer(() => this.fire(), Math.max(0, deadline - this.now()));
  }

  private disarm(): void {
    if (this.handle !== null) this.clearTimer(this.handle);
    this.handle = null;
  }

  private fire(): void {
    this.handle = null;
    // the timer was set for the deadline, but edits may have moved it
    if (!this.timer.due(this.now())) {
      this.arm();
      return;
    }
    this.timer.reset();
    void this.start().catch(() => {
      // the save reports its own failure; a timer has nobody to tell
    });
  }

  private start(): Promise<void> {
    const run = (async () => {
      try {
        await this.save();
      } finally {
        this.running = null;
        if (this.again) {
          this.again = false;
          this.timer.touch(this.now());
          this.arm();
        }
      }
    })();
    this.running = run;
    return run;
  }
}

// One shared clock so relative times ("12s", "saved 4m ago") refresh without per-component timers.

const TICK_MS = 5000;

class Clock {
  now = $state(Date.now());
  private timer: ReturnType<typeof setInterval> | null = null;

  start(): void {
    if (this.timer !== null) return;
    this.timer = setInterval(() => {
      this.now = Date.now();
    }, TICK_MS);
  }

  /** Brings `now` up to date immediately (after a user action that shows times). */
  touch(): void {
    this.now = Date.now();
  }
}

export const clock = new Clock();

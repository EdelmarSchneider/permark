// Tells a long press from a click on a control: holding the main button (or a finger) down for a moment runs
// `onLong`, and the click that ends the press is then not a click.

export const LONG_PRESS_MS = 500;

/** What a press needs from the pointer event that starts it. */
type PressStart = { button: number; currentTarget: EventTarget | null };

export class LongPress {
  private timer: ReturnType<typeof setTimeout> | null = null;
  private fired = false;

  constructor(
    private readonly onLong: (target: HTMLElement) => void,
    private readonly delay = LONG_PRESS_MS,
  ) {}

  /** The pointer went down on the control. Only the main button starts a press (the other one is the context menu). */
  down(event: PressStart): void {
    this.cancel();
    this.fired = false;
    const target = event.currentTarget;
    if (event.button !== 0 || !(target instanceof HTMLElement)) return;
    this.timer = setTimeout(() => {
      this.timer = null;
      this.fired = true;
      this.onLong(target);
    }, this.delay);
  }

  /** The pointer left the control or the system took the gesture: the press is over without being long. */
  cancel(): void {
    if (this.timer !== null) clearTimeout(this.timer);
    this.timer = null;
  }

  /**
   * The pointer came up. A press that was long leaves its click to be swallowed (see [`LongPress.swallowClick`]); the
   * click comes right after, so what is left of the press is forgotten once it has had its chance.
   */
  release(): void {
    this.cancel();
    if (this.fired) setTimeout(() => (this.fired = false), 0);
  }

  /** Whether the click that is arriving ends a long press, and so must not do what a click does. */
  swallowClick(): boolean {
    const was = this.fired;
    this.fired = false;
    return was;
  }
}

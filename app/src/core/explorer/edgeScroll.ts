// A drag near the top | bottom edge of a list scrolls it, one step per frame while the pointer stays in the edge zone
// (the Explorer's own drag and a drag from outside alike). The frame loop stops by itself outside the zone.

import { edgeSpeed } from "./list";

export type FrameClock = {
  request: (run: () => void) => number;
  cancel: (id: number) => void;
};

const browserClock: FrameClock = {
  request: (run) => requestAnimationFrame(run),
  cancel: (id) => cancelAnimationFrame(id),
};

export class EdgeScroll {
  readonly #list: HTMLElement;
  readonly #clock: FrameClock;
  #y: number | null = null;
  #frame: number | null = null;

  constructor(list: HTMLElement, clock: FrameClock = browserClock) {
    this.#list = list;
    this.#clock = clock;
  }

  /** The pointer is at height `y` (viewport px); `null`: the drag is over (the loop stops). */
  point(y: number | null): void {
    this.#y = y;
    if (y === null) {
      this.stop();
      return;
    }
    if (this.#frame === null && this.#speed() !== 0) this.#frame = this.#clock.request(this.#tick);
  }

  stop(): void {
    if (this.#frame !== null) this.#clock.cancel(this.#frame);
    this.#frame = null;
    this.#y = null;
  }

  #speed(): number {
    if (this.#y === null) return 0;
    const box = this.#list.getBoundingClientRect();
    return edgeSpeed(this.#y, box.top, box.bottom);
  }

  readonly #tick = (): void => {
    this.#frame = null;
    const speed = this.#speed();
    if (speed === 0) return;
    this.#list.scrollTop += speed;
    this.#frame = this.#clock.request(this.#tick);
  };
}

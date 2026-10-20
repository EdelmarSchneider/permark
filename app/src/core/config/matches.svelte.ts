// How many project files each pattern of an ignore file matches: asked once the sheet is shown, again (after a short
// pause) when the text changes. An answer for an older text or another file is dropped.

import { api } from "../../api";

const PAUSE_MS = 400;

export class Matches {
  /** Files per pattern line; none until the first answer. */
  counts = $state.raw<ReadonlyMap<number, number> | null>(null);
  /** The walk was cut: the counts are lower bounds. */
  capped = $state(false);

  #timer: ReturnType<typeof setTimeout> | null = null;
  #seq = 0;

  /** Asks for the counts of `path` (`text`: the editor's version, else the disk's). */
  request(path: string, text: string | null, now = false): void {
    if (this.#timer !== null) clearTimeout(this.#timer);
    const seq = ++this.#seq;
    const run = async (): Promise<void> => {
      this.#timer = null;
      const result = await api.ignoreMatches(path, text);
      if (seq !== this.#seq) return;
      if (!result.ok) {
        this.counts = null;
        return;
      }
      this.counts = new Map(result.value.counts.map((c) => [c.line, c.count]));
      this.capped = result.value.capped;
    };
    if (now) void run();
    else this.#timer = setTimeout(() => void run(), PAUSE_MS);
  }

  /** Nothing more is wanted (the sheet closed). */
  stop(): void {
    if (this.#timer !== null) clearTimeout(this.#timer);
    this.#timer = null;
    this.#seq += 1;
  }
}

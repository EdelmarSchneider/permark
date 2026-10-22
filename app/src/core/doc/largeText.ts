// A large text file in the viewer: the lines held so far (the first part, then the chunks that came), and when the
// next chunk is asked for. Pure: the viewer owns the state and the requests.

import type { TextChunk } from "../../api";

/** The next chunk is asked for when the lines in view come this close to the end of the lines held. */
export const FAR_LINES = 2000;
/** At most this much text is held; past it the rest is left to the system's program. */
export const MAX_HELD_CHARS = 64 * 1024 * 1024;

/** What the loading of the next chunk is doing. */
export type Load =
  | { status: "idle" }
  | { status: "loading" }
  | { status: "failed" }
  /** Enough is held. */
  | { status: "capped" }
  /** The file has no more. */
  | { status: "done" };

export type Held = {
  /** What it was made for (a new view of the file starts again). */
  owner: object;
  /** The lines (the empty piece after a final line break is no line). */
  lines: readonly string[];
  /** The text held does not end with a line break: the next chunk goes on with the last line. */
  open: boolean;
  /** Characters held. */
  chars: number;
  /** Where the next chunk starts (`null`: no more). */
  next: number | null;
  load: Load;
};

/** The lines of a text, and whether its last line is unfinished. */
export function linesOf(text: string): { lines: string[]; open: boolean } {
  const lines = text.split("\n");
  const open = lines.at(-1) !== "";
  if (!open) lines.pop();
  return { lines, open };
}

/** What a view holds when it opens: its text, and where the rest starts (`null`: it is all there). */
export function start(owner: object, text: string, next: number | null): Held {
  const { lines, open } = linesOf(text);
  return {
    owner,
    lines,
    open,
    chars: text.length,
    next,
    load: next === null ? { status: "done" } : { status: "idle" },
  };
}

/** The lines with a chunk added (its first line goes on with the last one when that was unfinished). */
export function append(held: Held, chunk: TextChunk): Held {
  const more = linesOf(chunk.text);
  let lines: readonly string[];
  if (held.open && held.lines.length > 0 && more.lines.length > 0) {
    lines = [
      ...held.lines.slice(0, -1),
      (held.lines.at(-1) ?? "") + (more.lines[0] ?? ""),
      ...more.lines.slice(1),
    ];
  } else {
    lines = [...held.lines, ...more.lines];
  }
  const chars = held.chars + chunk.text.length;
  // a chunk that brought nothing and did not move on ends the file (a loop would never end)
  const moved = chunk.next !== null && chunk.text.length > 0 && chunk.next > (held.next ?? 0);
  const next = moved ? chunk.next : null;
  return {
    ...held,
    lines,
    open: more.lines.length === 0 ? held.open : more.open,
    chars,
    next,
    load:
      next === null
        ? { status: "done" }
        : chars >= MAX_HELD_CHARS
          ? { status: "capped" }
          : { status: "idle" },
  };
}

/**
 * Whether the next chunk is wanted: the lines in view (`first` + `visible`) are close to the end of what is held.
 * `total`: the rows held, when a row is not a line (wrapped lines).
 */
export function wantsMore(
  held: Held,
  first: number,
  visible: number,
  total: number = held.lines.length,
): boolean {
  return held.load.status === "idle" && held.next !== null && first + visible >= total - FAR_LINES;
}

/** The same with its load changed. */
export function loading(held: Held, load: Load): Held {
  return { ...held, load };
}

/** Megabytes for a note (whole numbers; a file is "large" already). */
export function megabytes(bytes: number): number {
  return Math.max(1, Math.round(bytes / (1024 * 1024)));
}

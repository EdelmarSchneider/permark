// Which folders and files the person silenced. The `quiet` plugin feeds it (empty while the plugin is off); the plugins
// that call for attention (unread, feed, follow, git) and the Explorer ask it. A mark covers the path and what is
// inside it; a closer mark wins; a linked worktree (`@wt/<name>`) is a root of its own.

export type Interest = "quiet" | "speaking";
export type Marks = Readonly<Record<string, Interest | undefined>>;

const WORKTREE = "@wt/";

/** The folder that contains `path`, or `null` at a root (the project, or a worktree). */
function parentOf(path: string): string | null {
  if (path === "") return null;
  if (path.startsWith(WORKTREE) && !path.slice(WORKTREE.length).includes("/")) return null;
  const at = path.lastIndexOf("/");
  return at < 0 ? "" : path.slice(0, at);
}

/** The closest mark on `path` or a folder above it decides; no mark means speaking. */
export function isQuiet(path: string, marks: Marks): boolean {
  let current: string | null = path;
  while (current !== null) {
    const mark = Object.hasOwn(marks, current) ? marks[current] : undefined;
    if (mark !== undefined) return mark === "quiet";
    current = parentOf(current);
  }
  return false;
}

/** The mark paths (of `before` and `after`) whose silence ended: they speak now and were quiet. */
export function wakingMarks(before: Marks, after: Marks): string[] {
  const paths = new Set([...Object.keys(before), ...Object.keys(after)]);
  return [...paths].filter((p) => isQuiet(p, before) && !isQuiet(p, after)).sort();
}

export class SilenceStore {
  #marks = $state.raw<Marks>({});
  readonly #wake = new Set<(paths: readonly string[]) => void>();

  get marks(): Marks {
    return this.#marks;
  }

  /** Reactive: whoever reads it follows the marks. */
  isSilenced(path: string): boolean {
    return isQuiet(path, this.#marks);
  }

  /** The plugin tells the marks as they are now; those that stopped being silent are announced to `onWake`. */
  set(next: Marks): void {
    const waking = wakingMarks(this.#marks, next);
    this.#marks = next;
    if (waking.length > 0) for (const handler of this.#wake) handler(waking);
  }

  /** The plugin is off: nothing is silenced, nobody is told (the marks are still the person's, in their file). */
  clear(): void {
    this.#marks = {};
  }

  /** Paths (folders | files) that were silenced and now speak: changes made meanwhile should not arrive at once. */
  onWake(handler: (paths: readonly string[]) => void): () => void {
    this.#wake.add(handler);
    return () => this.#wake.delete(handler);
  }
}

export const silence = new SilenceStore();

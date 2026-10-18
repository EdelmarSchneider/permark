// The folders and files this person silenced. The backend keeps them; the screens ask `isQuiet`.

import { api } from "../api";
import { isQuietPath, type Interest } from "../quiet";

class QuietStore {
  marks = $state.raw<ReadonlyMap<string, Interest>>(new Map());
  /** Search leaves silenced paths out until this is on. */
  includeQuiet = $state(false);
  private listeners = new Set<() => void>();

  /** Told after the marks change, so a list can drop what just became quiet. */
  subscribe(fn: () => void): void {
    this.listeners.add(fn);
  }

  private emit(): void {
    for (const fn of this.listeners) fn();
  }

  isQuiet(path: string): boolean {
    return isQuietPath(path, this.marks);
  }

  /** A project was opened or closed. */
  reset(): void {
    this.marks = new Map();
    this.includeQuiet = false;
  }

  async refresh(): Promise<void> {
    try {
      const rules = await api.interestRules();
      this.marks = new Map(Object.entries(rules));
      this.emit();
    } catch {
      // the tree still works; a later set asks again
    }
  }

  /** Silences `paths`, or makes them speak. The answer is the whole set of marks. */
  async set(paths: readonly string[], interest: Interest): Promise<void> {
    const rules = await api.setInterest([...paths], interest);
    this.marks = new Map(Object.entries(rules));
    this.emit();
  }

  toggleInclude(): void {
    this.includeQuiet = !this.includeQuiet;
  }
}

export const quiet = new QuietStore();

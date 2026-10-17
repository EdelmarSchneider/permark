// The page's remembered choices were kept under `trailmark.*` before the app was renamed. They are copied to `permark.*`
// once, at the start, so nobody loses a hint they dismissed or a preference they set. The old keys stay where they are.

const OLD_PREFIX = "trailmark.";
const NEW_PREFIX = "permark.";

/** Copies every `trailmark.*` entry to its `permark.*` key when that key is not set yet. Never throws. */
export function migrateLegacyStorage(storage?: Storage): void {
  try {
    const store = storage ?? localStorage;
    const old: string[] = [];
    for (let i = 0; i < store.length; i += 1) {
      const key = store.key(i);
      if (key?.startsWith(OLD_PREFIX)) old.push(key);
    }
    for (const key of old) {
      const target = NEW_PREFIX + key.slice(OLD_PREFIX.length);
      if (store.getItem(target) !== null) continue;
      const value = store.getItem(key);
      if (value !== null) store.setItem(target, value);
    }
  } catch {
    // storage blocked or full: the app works without it
  }
}

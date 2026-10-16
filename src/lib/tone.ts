// The tone of the light theme: plain white, or a softer paper, sage or gray for tired eyes. The theme (System, Light,
// Dark) stays its own choice; the tone only paints the light theme, so it has no effect while the theme is dark. The
// page carries it as `data-tone` on <html>, next to `data-theme`, and `styles/tokens.css` does the painting.

export const TONES = ["white", "paper", "sage", "gray"] as const;
export type Tone = (typeof TONES)[number];

export const DEFAULT_TONE: Tone = "white";

/** Where the last tone is remembered by the page, so the next start paints it before the backend has answered. */
export const TONE_CACHE_KEY = "trailmark.tone";

/** A stored value as a tone: anything this build does not know reads as the default. */
export function normalizeTone(value: unknown): Tone {
  return TONES.find((tone) => tone === value) ?? DEFAULT_TONE;
}

/** Puts the tone on the page. */
export function applyTone(root: HTMLElement, tone: Tone): void {
  root.dataset.tone = tone;
}

/** The page's own storage, when the browser lets it be used (it can be blocked, and the accessor itself can throw). */
export function pageStorage(): Storage | undefined {
  try {
    return typeof localStorage === "undefined" ? undefined : localStorage;
  } catch {
    return undefined;
  }
}

/** The tone remembered by this page, or the default (the storage can be missing or blocked). */
export function cachedTone(storage: Pick<Storage, "getItem"> | undefined): Tone {
  try {
    return normalizeTone(storage?.getItem(TONE_CACHE_KEY));
  } catch {
    return DEFAULT_TONE;
  }
}

/** Remembers the tone for the next start; a storage that refuses is not worth an error. */
export function cacheTone(storage: Pick<Storage, "setItem"> | undefined, tone: Tone): void {
  try {
    storage?.setItem(TONE_CACHE_KEY, tone);
  } catch {
    // the next start waits for the backend instead
  }
}

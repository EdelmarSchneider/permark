// What diagram embeds (Mermaid, PlantUML, Graphviz) share: one render queue (a preview and a document never draw at
// the same time), a small result cache, the page's palette, and the cleaning every drawing gets before it enters the
// page.

/** A drawing that could not be made, with the words that say why (the engine's, or why it was not tried). */
export type DrawFailure = { problem: string };

const REMOTE_LINK = /^(?:javascript:|data:|vbscript:)/i;
const REMOTE_ASSET = /^(?:javascript:|data:|vbscript:|https?:|\/\/)/i;
const CACHE_KEPT = 60;

/** The XML parser takes an attribute once; some engines write `xmlns` twice. */
function uniqueXmlns(svg: string): string {
  let seen = false;
  return svg.replace(/\sxmlns="[^"]*"/g, (match) => {
    if (seen) return "";
    seen = true;
    return match;
  });
}

/** The drawing with scripts, foreign content, event handlers and remote references removed; null when it is not SVG. */
export function sanitizeSvg(svg: string): string | null {
  const doc = new DOMParser().parseFromString(uniqueXmlns(svg), "image/svg+xml");
  const root = doc.documentElement;
  if (root.localName !== "svg" || doc.querySelector("parsererror")) return null;
  root
    .querySelectorAll(
      "script, foreignObject, iframe, object, embed, style[src], set, animate, animateMotion, animateTransform, discard",
    )
    .forEach((el) => el.remove());
  for (const el of root.querySelectorAll("*")) {
    for (const attr of [...el.attributes]) {
      if (attr.name.toLowerCase().startsWith("on")) el.removeAttribute(attr.name);
    }
    const href = el.getAttribute("href") ?? el.getAttribute("xlink:href");
    if (href === null) continue;
    // `java\tscript:` is still a script address to a browser: spaces and control characters do not count
    const value = href.replace(/[\u0000-\u0020]/g, "");
    const link = el.localName === "a";
    if (link ? REMOTE_LINK.test(value) : REMOTE_ASSET.test(value)) {
      if (link) {
        el.removeAttribute("href");
        el.removeAttribute("xlink:href");
      } else {
        el.remove();
      }
    }
  }
  return new XMLSerializer().serializeToString(root);
}

/** The width a drawing was made at (from its viewBox), so the figure can size to it. */
export function viewBoxWidth(viewBox: string | null | undefined): number | null {
  const parts = (viewBox ?? "")
    .trim()
    .split(/[\s,]+/)
    .map(Number);
  const width = parts[2];
  return parts.length === 4 && width !== undefined && Number.isFinite(width) && width > 0
    ? width
    : null;
}

/** A forced theme ignores the page's light tone: its drawings never share a cache entry with the page's. */
export function paletteKey(theme: "light" | "dark", forced: boolean): string {
  return forced ? `${theme}:forced` : `${theme}:${document.documentElement.dataset["tone"] ?? ""}`;
}

/**
 * `read` gets the design tokens: the page's (theme + tone), or for a forced theme those of a probe element scoped to
 * that theme alone (the tokens scope each theme by `data-theme`; the tones apply to the root only).
 */
export function withTokens<T>(
  theme: "light" | "dark",
  forced: boolean,
  read: (token: (name: string) => string) => T,
): T {
  const reader = (element: Element) => {
    const css = getComputedStyle(element);
    return (name: string): string => css.getPropertyValue(name).trim();
  };
  if (!forced) return read(reader(document.documentElement));
  const probe = document.createElement("div");
  probe.hidden = true;
  probe.dataset["theme"] = theme;
  document.body.append(probe);
  try {
    // read before the probe leaves: a detached element has no computed tokens
    return read(reader(probe));
  } finally {
    probe.remove();
  }
}

class DiagramQueue {
  #tail: Promise<unknown> = Promise.resolve();
  #cache = new Map<string, string>();

  /**
   * The drawing for `key`: cached, else made by `job` after the ones queued before it. `null` | a failure: it cannot
   * be drawn (never cached: the next ask tries again).
   */
  draw<R extends string | DrawFailure | null>(
    key: string,
    job: () => Promise<R>,
  ): Promise<R | string | null> {
    const cached = this.#cache.get(key);
    if (cached !== undefined) return Promise.resolve(cached);
    const run = this.#tail.then(async () => {
      try {
        const svg = await job();
        if (typeof svg !== "string") return svg;
        if (this.#cache.size >= CACHE_KEPT) this.#cache.clear();
        this.#cache.set(key, svg);
        return svg;
      } catch {
        return null;
      }
    });
    this.#tail = run.then(
      () => undefined,
      () => undefined,
    );
    return run;
  }

  /** Forgets every drawing (tests). */
  clear(): void {
    this.#cache.clear();
  }
}

export const diagrams = new DiagramQueue();

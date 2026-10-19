// How this window reads: width, zoom, the outline panel, where each document was left. Kept per project in the
// project session (a single file keeps them in memory only).

import type { Api, JsonValue } from "../../api";

export type Width = "comfortable" | "wide" | "full";
export const WIDTHS: readonly Width[] = ["comfortable", "wide", "full"];

export const ZOOM_MIN = 50;
export const ZOOM_MAX = 200;
export const ZOOM_STEP = 10;
export const ZOOM_DEFAULT = 100;

const KEYS = {
  width: "reading.width",
  zoom: "reading.zoom",
  outline: "reading.outline",
  scroll: "reading.scroll",
} as const;

/** Documents whose reading place is kept. */
const SCROLL_KEPT = 50;

export function clampZoom(percent: number): number {
  if (!Number.isFinite(percent)) return ZOOM_DEFAULT;
  const stepped = Math.round(percent / ZOOM_STEP) * ZOOM_STEP;
  return Math.min(ZOOM_MAX, Math.max(ZOOM_MIN, stepped));
}

function isWidth(value: unknown): value is Width {
  return WIDTHS.some((w) => w === value);
}

function parseScroll(value: JsonValue | null): Record<string, number> {
  if (typeof value !== "object" || value === null || Array.isArray(value)) return {};
  const out: Record<string, number> = {};
  for (const [path, top] of Object.entries(value)) {
    if (typeof top === "number" && Number.isFinite(top) && top >= 0) out[path] = top;
  }
  return out;
}

class ReadingStore {
  width = $state<Width>("comfortable");
  zoom = $state(ZOOM_DEFAULT);
  /** The outline panel is open. */
  outline = $state(true);
  #scroll = new Map<string, number>();
  #api: Api | null = null;
  #saveTimer: ReturnType<typeof setTimeout> | null = null;

  /** Reads the project's choices (a single file or a failure leaves the defaults). */
  async load(api: Api): Promise<void> {
    this.#api = api;
    this.width = "comfortable";
    this.zoom = ZOOM_DEFAULT;
    this.outline = true;
    this.#scroll = new Map();
    const [width, zoom, outline, scroll] = await Promise.all([
      api.projectSessionGet(KEYS.width),
      api.projectSessionGet(KEYS.zoom),
      api.projectSessionGet(KEYS.outline),
      api.projectSessionGet(KEYS.scroll),
    ]);
    if (api !== this.#api) return;
    if (width.ok && isWidth(width.value)) this.width = width.value;
    if (zoom.ok && typeof zoom.value === "number") this.zoom = clampZoom(zoom.value);
    if (outline.ok && typeof outline.value === "boolean") this.outline = outline.value;
    if (scroll.ok) this.#scroll = new Map(Object.entries(parseScroll(scroll.value)));
  }

  setWidth(width: Width): void {
    this.width = width;
    this.#save(KEYS.width, width);
  }

  setZoom(percent: number): void {
    this.zoom = clampZoom(percent);
    this.#save(KEYS.zoom, this.zoom);
  }

  zoomBy(steps: number): void {
    this.setZoom(this.zoom + steps * ZOOM_STEP);
  }

  toggleOutline(): void {
    this.outline = !this.outline;
    this.#save(KEYS.outline, this.outline);
  }

  /** Where the document was left (pixels from the top), if it was. */
  scrollOf(path: string): number | null {
    return this.#scroll.get(path) ?? null;
  }

  /** Remembers where a document is left; written out a moment later. */
  remember(path: string, top: number): void {
    this.#scroll.delete(path);
    if (top > 0) this.#scroll.set(path, Math.round(top));
    while (this.#scroll.size > SCROLL_KEPT) {
      const oldest = this.#scroll.keys().next().value;
      if (oldest === undefined) break;
      this.#scroll.delete(oldest);
    }
    if (this.#saveTimer !== null) clearTimeout(this.#saveTimer);
    this.#saveTimer = setTimeout(() => {
      this.#saveTimer = null;
      this.#save(KEYS.scroll, Object.fromEntries(this.#scroll));
    }, 800);
  }

  #save(key: string, value: JsonValue): void {
    // a single file has no project session: the error is the answer, nothing to tell the person
    void this.#api?.projectSessionSet(key, value);
  }
}

export const reading = new ReadingStore();
export type { ReadingStore };

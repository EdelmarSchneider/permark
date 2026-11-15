// Files an export page carries inside itself as data URIs (pictures, sound, video, fonts), so it needs nothing else
// to show: no project folder, no network.

/** Biggest file (picture, sound, video) an export page carries inside itself. */
export const MEDIA_MAX_BYTES = 25 * 1024 * 1024;

/** Bytes turned into text per step (the argument list of `String.fromCharCode` stays small). */
const CHUNK = 0x8000;

export type Fetched = { status: "ok"; uri: string } | { status: "tooLarge" } | { status: "failed" };

function base64(bytes: Uint8Array): string {
  const parts: string[] = [];
  for (let at = 0; at < bytes.length; at += CHUNK) {
    parts.push(String.fromCharCode(...bytes.subarray(at, at + CHUNK)));
  }
  return btoa(parts.join(""));
}

export function dataUri(bytes: Uint8Array, mime: string): string {
  return `data:${mime};base64,${base64(bytes)}`;
}

/** An SVG file shows only when its type says so; other pictures are recognized by the browser from their bytes. */
function mimeOf(response: Response, name: string): string {
  const type = (response.headers.get("content-type") ?? "").split(";")[0]?.trim() ?? "";
  if (type !== "") return type;
  return /\.svg$/i.test(name) ? "image/svg+xml" : "application/octet-stream";
}

/** The body of `response`, or null once it passes `max` bytes (the rest is never read). */
async function readUpTo(response: Response, max: number): Promise<Uint8Array | null> {
  if (response.body === null) return new Uint8Array(0);
  const reader = response.body.getReader();
  const chunks: Uint8Array[] = [];
  let size = 0;
  for (;;) {
    const { done, value } = await reader.read();
    if (done) break;
    size += value.byteLength;
    if (size > max) {
      void reader.cancel();
      return null;
    }
    chunks.push(value);
  }
  const bytes = new Uint8Array(size);
  let at = 0;
  for (const chunk of chunks) {
    bytes.set(chunk, at);
    at += chunk.byteLength;
  }
  return bytes;
}

/**
 * The file at `url` (the page's own asset URL | a `blob:` URL the window already holds) as a data URI. A file over
 * `max` bytes (default [`MEDIA_MAX_BYTES`]) is not read: its declared size decides when the answer gives one, else
 * reading stops once it passes the limit.
 */
export async function fetchDataUri(
  url: string,
  options: { max?: number; signal?: AbortSignal | undefined } = {},
): Promise<Fetched> {
  if (url.startsWith("data:")) return { status: "ok", uri: url };
  const max = options.max ?? MEDIA_MAX_BYTES;
  let response: Response;
  try {
    response = await fetch(url, options.signal ? { signal: options.signal } : undefined);
  } catch {
    options.signal?.throwIfAborted();
    return { status: "failed" };
  }
  if (!response.ok) return { status: "failed" };
  const declared = Number(response.headers.get("content-length"));
  if (Number.isFinite(declared) && declared > max) {
    void response.body?.cancel();
    return { status: "tooLarge" };
  }
  const bytes = await readUpTo(response, max);
  if (bytes === null) return { status: "tooLarge" };
  return { status: "ok", uri: dataUri(bytes, mimeOf(response, url)) };
}

/**
 * Which of the font files named by `stems` (names without extension) a URL points at: the source path (dev server)
 * or a built asset, whose name is the stem, a dash and a content hash (which may hold dashes too: the longest stem
 * that fits wins, never a shorter name's file). Null: none of them.
 */
export function fontStem(url: string, stems: Iterable<string>): string | null {
  const file = url.split(/[?#]/)[0]?.split("/").pop() ?? "";
  if (!file.endsWith(".woff2")) return null;
  let best: string | null = null;
  for (const stem of stems) {
    const fits = file === `${stem}.woff2` || file.startsWith(`${stem}-`);
    if (fits && (best === null || stem.length > best.length)) best = stem;
  }
  return best;
}

/** `css` with every `url(...)` of a font file in `files` (by stem, see [`fontStem`]) replaced by its data URI. */
export function inlineFontUrls(css: string, files: ReadonlyMap<string, string>): string {
  return css.replace(/url\(\s*(["']?)([^"')]+)\1\s*\)/g, (whole, _quote: string, url: string) => {
    const stem = fontStem(url, files.keys());
    const uri = stem === null ? undefined : files.get(stem);
    return uri === undefined ? whole : `url("${uri}")`;
  });
}

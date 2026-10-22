// Pictures pasted | dropped into an editor: which files count, their type, the size rules, the link written; and
// where a picture of the document comes from when Edit draws it (the reading view's rule, `sanitize.rs`).

/** Asks first above this size. */
export const ASK_ABOVE = 5 * 1024 * 1024;
/** Refused above this size (the backend refuses too). */
export const REFUSE_ABOVE = 50 * 1024 * 1024;

const TYPES: Record<string, string> = {
  "image/png": "png",
  "image/jpeg": "jpg",
  "image/gif": "gif",
  "image/webp": "webp",
  "image/svg+xml": "svg",
};

/** The pictures of a paste | drop the app saves (PNG, JPG, GIF, WebP, SVG). */
export function pictureFiles(files: readonly File[]): File[] {
  return files.filter((file) => TYPES[file.type] !== undefined);
}

export function extensionOf(file: File): string | null {
  return TYPES[file.type] ?? null;
}

/** The link to a picture saved next to the document: its name, spaces and brackets encoded. */
export function pictureLink(doc: string, saved: string): string {
  const docDir = doc.split("/").slice(0, -1).join("/");
  const savedDir = saved.split("/").slice(0, -1).join("/");
  const name = saved.split("/").at(-1) ?? saved;
  const encoded = name.replace(/ /g, "%20").replace(/[()<>]/g, encodeURIComponent);
  return docDir === savedDir ? encoded : saved.replace(/ /g, "%20");
}

/** Where a picture of the document comes from, as Edit draws it. */
export type PictureSource =
  /** A file of the project (project path), loaded through the asset scheme. */
  | { kind: "file"; path: string }
  /** A raster picture written inline (`data:image/png;...`). */
  | { kind: "inline"; src: string }
  /** A picture on the web: not fetched (offline by design). */
  | { kind: "web"; url: string }
  /** Nothing to load: empty, above the project root, or a scheme pictures do not take. */
  | { kind: "none" };

const WORKTREE_PREFIX = "@wt/";
const RASTER_DATA = /^data:image\/(?:png|jpeg|gif|webp)[;,]/;
const SCHEME = /^[a-z][a-z0-9+.-]*:/;

/** The value as the URL parser reads its scheme: control characters and spaces dropped, case folded. */
function normalized(value: string): string {
  let out = "";
  for (const c of value) if (!/[\p{Cc}\s]/u.test(c)) out += c;
  return out.toLowerCase();
}

/** `%XX` decoded byte by byte; a broken sequence stays as written (`links::percent_decode`). */
function percentDecode(text: string): string {
  if (!text.includes("%")) return text;
  const bytes = new TextEncoder().encode(text);
  const out: number[] = [];
  const hex = (b: number | undefined): number =>
    b === undefined ? -1 : Number.parseInt(String.fromCharCode(b), 16);
  for (let i = 0; i < bytes.length; i++) {
    const byte = bytes[i] ?? 0;
    const hi = hex(bytes[i + 1]);
    const lo = hex(bytes[i + 2]);
    if (byte === 0x25 && hi >= 0 && lo >= 0) {
      out.push(hi * 16 + lo);
      i += 2;
    } else out.push(byte);
  }
  return new TextDecoder().decode(new Uint8Array(out));
}

/** `link` joined to the folder of `doc` (a leading `/` = the root of the doc's tree); null above that root. */
function joinRelative(doc: string, link: string): string | null {
  const from = doc.replaceAll("\\", "/");
  let prefix = "";
  let inner = from;
  if (from.startsWith(WORKTREE_PREFIX)) {
    const rest = from.slice(WORKTREE_PREFIX.length);
    const slash = rest.indexOf("/");
    const name = slash < 0 ? rest : rest.slice(0, slash);
    prefix = `${WORKTREE_PREFIX}${name}/`;
    inner = slash < 0 ? "" : rest.slice(slash + 1);
  }
  const parts = link.startsWith("/") ? [] : inner.split("/").filter((s) => s !== "");
  if (!link.startsWith("/")) parts.pop(); // the document's own name
  for (const segment of link.split("/")) {
    if (segment === "" || segment === ".") continue;
    if (segment === "..") {
      if (parts.pop() === undefined) return null;
    } else parts.push(segment);
  }
  return prefix + parts.join("/");
}

/**
 * Where the picture `src` of the document at project path `doc` comes from: the same rule as the reading view
 * (relative -> the file next to the document; above the root -> nothing; raster `data:` kept; web -> not fetched).
 */
export function pictureSource(doc: string, src: string): PictureSource {
  const value = src.trim();
  const plain = normalized(value);
  if (plain.startsWith("data:")) {
    return RASTER_DATA.test(plain) ? { kind: "inline", src: value } : { kind: "none" };
  }
  if (value === "" || value.startsWith("#")) return { kind: "none" };
  if (value.startsWith("//") || /^https?:/.test(plain)) return { kind: "web", url: value };
  if (SCHEME.test(plain)) return { kind: "none" };
  const path = percentDecode(value.split(/[#?]/)[0] ?? value)
    .replaceAll("\\", "/")
    .trim();
  if (path === "") return { kind: "none" };
  const joined = joinRelative(doc, path);
  return joined === null || joined === "" ? { kind: "none" } : { kind: "file", path: joined };
}

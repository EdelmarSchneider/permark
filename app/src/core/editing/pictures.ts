// Pictures pasted | dropped into an editor: which files count, the clipboard read, their type, the size rules,
// the link written, the address Edit paints (the markdown path stays relative).

import { assetPathOf } from "../doc/enhance";
import { resolveLink } from "./source/complete";

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

/** Enough of a paste clipboard to read `files` and file `items` (`getAsFile`). */
type PasteClipboard = {
  readonly files: ArrayLike<File>;
  readonly items: ArrayLike<{
    readonly kind: string;
    getAsFile(): File | null;
  }>;
};

/**
 * Accepted pictures on a paste: `files`, plus file items (`kind === "file"`, `getAsFile`).
 * An item whose type and size match a file already taken is left out, so that picture is saved once.
 */
export function clipboardPictures(data: PasteClipboard | null): File[] {
  if (data === null) return [];
  const fromFiles = pictureFiles(Array.from(data.files));
  const extra: File[] = [];
  for (const item of Array.from(data.items)) {
    if (item.kind !== "file") continue;
    const file = item.getAsFile();
    if (file === null || extensionOf(file) === null) continue;
    const already = fromFiles.some((have) => have.type === file.type && have.size === file.size);
    if (already) continue;
    extra.push(file);
  }
  return fromFiles.concat(extra);
}

/** Markdown link of a saved picture, relative to the document. Spaces and `()<>` are encoded. `..` is not. */
export function pictureLink(doc: string, saved: string): string {
  const from = doc.split("/").slice(0, -1);
  const to = saved.split("/");
  let i = 0;
  while (i < from.length && i < to.length - 1 && from[i] === to[i]) i += 1;
  const up = from.slice(i).map(() => "..");
  const rest = to.slice(i);
  return [...up, ...rest].map(encodeSegment).join("/");
}

/** `encodeURIComponent` leaves `()` alone; a markdown destination cannot. */
const ESCAPE: Record<string, string> = {
  " ": "%20",
  "(": "%28",
  ")": "%29",
  "<": "%3C",
  ">": "%3E",
};

function encodeSegment(segment: string): string {
  return segment === ".." ? segment : segment.replace(/[ ()<>]/g, (ch) => ESCAPE[ch] ?? ch);
}

/** A scheme or a protocol-relative address: the browser loads it. `pm-asset:` is not one of these. */
const REMOTE = /^(?:[a-z][a-z0-9+.-]*:|\/\/)/i;

/**
 * Address for an `<img>` in Edit. `src` stays the markdown path on the node.
 * A local path (and `pm-asset:`) becomes `asset(vaultPath)`. `http`, `data:`, `blob:` stay.
 * A path that climbs out of the project stays as written.
 */
export function pictureDisplayUrl(
  doc: string,
  src: string,
  asset: (vaultPath: string) => string,
): string {
  const trimmed = src.trim();
  if (trimmed === "") return "";
  const fromAsset = assetPathOf(trimmed);
  if (fromAsset !== null) return fromAsset === "" ? trimmed : asset(fromAsset);
  if (REMOTE.test(trimmed)) return trimmed;
  // a leading `/` is the project root, the same rule as the reading view
  const vault = resolveLink(trimmed.startsWith("/") ? "_.md" : doc, trimmed);
  return vault === null || vault === "" ? trimmed : asset(vault);
}

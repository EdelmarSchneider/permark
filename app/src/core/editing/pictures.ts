// Pictures pasted | dropped into an editor: which files count, the clipboard read, their type, the size rules,
// the link written.

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

/** The link to a picture saved next to the document: its name, spaces and brackets encoded. */
export function pictureLink(doc: string, saved: string): string {
  const docDir = doc.split("/").slice(0, -1).join("/");
  const savedDir = saved.split("/").slice(0, -1).join("/");
  const name = saved.split("/").at(-1) ?? saved;
  const encoded = name.replace(/ /g, "%20").replace(/[()<>]/g, encodeURIComponent);
  return docDir === savedDir ? encoded : saved.replace(/ /g, "%20");
}

// Pictures pasted | dropped into an editor: which files count, their type, the size rules, the link written.

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

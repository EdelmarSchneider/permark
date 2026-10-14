// Names for the images added to a document (docs/design/v0.2.md section 3, "Images"): a pasted picture is called
// `<doc-stem>-YYYYMMDD-HHMMSS.<ext>`, a dropped file keeps its own name. The backend makes the name safe and unique.

import { basename } from "../paths";

/** Largest image added without asking. */
export const CONFIRM_ABOVE_BYTES = 10 * 1024 * 1024;
/** Largest image added at all (the backend refuses more); a bigger file is never read. */
export const MAX_IMAGE_BYTES = 32 * 1024 * 1024;

/** A file size for a message: "12.3 MB". */
export function megabytes(bytes: number): string {
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

const TYPES: Record<string, string> = {
  "image/png": "png",
  "image/jpeg": "jpg",
  "image/gif": "gif",
  "image/webp": "webp",
  "image/svg+xml": "svg",
};
const EXTENSIONS = new Set(Object.values(TYPES).concat("jpeg"));

/** The extension for an image type, or null when it is not one we add. */
export function imageExtension(mime: string): string | null {
  return TYPES[mime.toLowerCase()] ?? null;
}

/** Whether a file name has one of the image extensions. */
export function isImageName(name: string): boolean {
  const dot = name.lastIndexOf(".");
  return dot > 0 && EXTENSIONS.has(name.slice(dot + 1).toLowerCase());
}

const two = (n: number): string => String(n).padStart(2, "0");

/** `live-feed-20261014-104201.png` for a picture pasted into `docs/specs/live-feed.md` at that local time. */
export function pastedImageName(docPath: string, extension: string, at: Date): string {
  const name = basename(docPath);
  const dot = name.lastIndexOf(".");
  const stem = dot > 0 ? name.slice(0, dot) : name;
  const date = `${at.getFullYear()}${two(at.getMonth() + 1)}${two(at.getDate())}`;
  const time = `${two(at.getHours())}${two(at.getMinutes())}${two(at.getSeconds())}`;
  return `${stem}-${date}-${time}.${extension}`;
}

/** The Markdown for an image: `![](assets/name.png)`; spaces and parentheses in the path are escaped. */
export function imageMarkdown(rel: string, alt = ""): string {
  const target = rel.replace(/ /g, "%20").replace(/\(/g, "%28").replace(/\)/g, "%29");
  return `![${alt}](${target})`;
}

/** Base64 of some bytes (the form the backend takes), without the `data:` prefix. */
export async function toBase64(blob: Blob): Promise<string> {
  const url = await new Promise<string>((resolve, reject) => {
    const reader = new FileReader();
    reader.onload = () => resolve(String(reader.result));
    reader.onerror = () => reject(reader.error ?? new Error("could not read the image"));
    reader.readAsDataURL(blob);
  });
  return url.slice(url.indexOf(",") + 1);
}

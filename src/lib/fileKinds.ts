// One table from file extension to category. The Explorer filter and Quick open use it to decide what is
// shown; a new format is one more extension here.

import { isMarkdown } from "./paths";

export const FILE_CATEGORIES = [
  "documents",
  "images",
  "media",
  "data",
  "xml",
  "text",
  "other",
] as const;
export type FileCategory = (typeof FILE_CATEGORIES)[number];

/** Pictures, sound and video are shown by the app itself. The lists mirror `src-tauri/src/media.rs`. */
const IMAGE_EXTENSIONS = [
  "png",
  "jpg",
  "jpeg",
  "gif",
  "webp",
  "avif",
  "bmp",
  "ico",
  "svg",
] as const;
/** Sound the webview plays, then the formats it may not (they are found and opened by name all the same). */
const AUDIO_EXTENSIONS = [
  "mp3",
  "wav",
  "ogg",
  "oga",
  "opus",
  "flac",
  "m4a",
  "aac",
  "weba",
  "wma",
  "aif",
  "aiff",
  "mka",
] as const;
const VIDEO_EXTENSIONS = [
  "mp4",
  "m4v",
  "webm",
  "ogv",
  "mov",
  "mkv",
  "avi",
  "wmv",
  "flv",
  "mpg",
  "mpeg",
  "3gp",
] as const;

const EXTENSIONS: Record<Exclude<FileCategory, "other">, readonly string[]> = {
  documents: ["md", "markdown", "mdown", "mkd", "mdx"],
  images: IMAGE_EXTENSIONS,
  media: [...AUDIO_EXTENSIONS, ...VIDEO_EXTENSIONS],
  data: ["json", "jsonc", "json5", "jsonl", "ndjson", "yaml", "yml", "toml", "csv", "tsv"],
  xml: ["xml", "xsd", "xsl", "xslt", "rss", "atom"],
  text: ["txt", "text", "rst", "adoc", "asciidoc", "org"],
};

const CATEGORY_BY_EXTENSION: ReadonlyMap<string, FileCategory> = new Map(
  (Object.entries(EXTENSIONS) as [FileCategory, readonly string[]][]).flatMap(([category, list]) =>
    list.map((extension) => [extension, category] as const),
  ),
);

/** The lower-case extension of a file name, without the dot; null for a name without one (and for a dotfile). */
export function extensionOf(path: string): string | null {
  const name = path.slice(path.lastIndexOf("/") + 1);
  const dot = name.lastIndexOf(".");
  if (dot <= 0 || dot === name.length - 1) return null;
  return name.slice(dot + 1).toLowerCase();
}

/** The category of a file, from the extension of its name (extensionless files and dotfiles are "other"). */
export function categoryOf(path: string): FileCategory {
  const extension = extensionOf(path);
  return extension === null ? "other" : (CATEGORY_BY_EXTENSION.get(extension) ?? "other");
}

export function isFileCategory(value: unknown): value is FileCategory {
  return typeof value === "string" && (FILE_CATEGORIES as readonly string[]).includes(value);
}

/** What a picture, sound or video is shown as. */
export type AssetKind = "image" | "audio" | "video";

const KIND_BY_EXTENSION: ReadonlyMap<string, AssetKind> = new Map([
  ...IMAGE_EXTENSIONS.map((e) => [e, "image"] as const),
  ...AUDIO_EXTENSIONS.map((e) => [e, "audio"] as const),
  ...VIDEO_EXTENSIONS.map((e) => [e, "video"] as const),
]);

/** Whether the file is a picture, a sound or a video, from its name; null for any other file. */
export function assetKindOf(path: string): AssetKind | null {
  const extension = extensionOf(path);
  return extension === null ? null : (KIND_BY_EXTENSION.get(extension) ?? null);
}

/** Whether the pane can show the file: a Markdown document, or a picture, a sound or a video. The rows of the feed use it. */
export function canOpenInPane(path: string): boolean {
  return isMarkdown(path) || assetKindOf(path) !== null;
}

/** Whether a document view is a picture, a sound or a video: drawn from the file itself, with no text to read. */
export function isMediaViewKind(kind: string): kind is AssetKind {
  return kind === "image" || kind === "audio" || kind === "video";
}

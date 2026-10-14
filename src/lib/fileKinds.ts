// One table from file extension to category. The Explorer filter and Quick open use it to decide what is
// shown; a new format is one more extension here.

export const FILE_CATEGORIES = ["documents", "images", "data", "xml", "text", "other"] as const;
export type FileCategory = (typeof FILE_CATEGORIES)[number];

const EXTENSIONS: Record<Exclude<FileCategory, "other">, readonly string[]> = {
  documents: ["md", "markdown", "mdown", "mkd", "mdx"],
  images: ["png", "jpg", "jpeg", "gif", "webp", "avif", "bmp", "ico", "svg"],
  data: ["json", "jsonc", "json5", "jsonl", "ndjson", "yaml", "yml", "toml", "csv", "tsv"],
  xml: ["xml", "xsd", "xsl", "xslt", "rss", "atom"],
  text: ["txt", "text", "rst", "adoc", "asciidoc", "org"],
};

const CATEGORY_BY_EXTENSION: ReadonlyMap<string, FileCategory> = new Map(
  (Object.entries(EXTENSIONS) as [FileCategory, readonly string[]][]).flatMap(([category, list]) =>
    list.map((extension) => [extension, category] as const),
  ),
);

/** The category of a file, from the extension of its name (extensionless files and dotfiles are "other"). */
export function categoryOf(path: string): FileCategory {
  const name = path.slice(path.lastIndexOf("/") + 1);
  const dot = name.lastIndexOf(".");
  if (dot <= 0) return "other";
  return CATEGORY_BY_EXTENSION.get(name.slice(dot + 1).toLowerCase()) ?? "other";
}

export function isFileCategory(value: unknown): value is FileCategory {
  return typeof value === "string" && (FILE_CATEGORIES as readonly string[]).includes(value);
}

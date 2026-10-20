// The front matter block of a Markdown document (`---` YAML | `+++` TOML) split into its parts, so the properties
// between the fences can be edited as a sheet and put back between the same fences.

export type FrontMatterParts = {
  syntax: "yaml" | "toml";
  /** The opening fence line. */
  open: string;
  /** The text between the fences (no final line break). */
  body: string;
  /** The closing fence line. */
  close: string;
};

/** `yaml` | `toml` for a block that starts with `---` | `+++`; none for anything else. */
export function frontMatterSyntax(source: string): "yaml" | "toml" | null {
  if (source.startsWith("---")) return "yaml";
  if (source.startsWith("+++")) return "toml";
  return null;
}

/** The block cut at its fences; none when it has no closing fence. */
export function splitFrontMatter(source: string): FrontMatterParts | null {
  const syntax = frontMatterSyntax(source);
  const lines = source.split("\n");
  if (syntax === null || lines.length < 2) return null;
  const open = lines[0] ?? "";
  const close = lines[lines.length - 1] ?? "";
  if (close.trim() !== open.trim()) return null;
  return { syntax, open, body: lines.slice(1, -1).join("\n"), close };
}

/** The block again with another text between the fences. */
export function joinFrontMatter(parts: FrontMatterParts, body: string): string {
  return [parts.open, ...(body === "" ? [] : [body]), parts.close].join("\n");
}

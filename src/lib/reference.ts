// Formatting of references that are pasted into an AI tool's terminal ("copy with reference").

export const DEFAULT_REF_TEMPLATE = "@{path}";

/** Applies the user's reference template; `{path}` is replaced (every occurrence). */
export function formatReference(template: string, path: string): string {
  const base = template.trim() === "" ? DEFAULT_REF_TEMPLATE : template;
  return base.split("{path}").join(path);
}

/** Collapses the selection into quote lines; blank lines stay as an empty quote line. */
export function quoteLines(text: string): string {
  return text
    .replace(/\r\n?/g, "\n")
    .trim()
    .split("\n")
    .map((line) => (line.trim() === "" ? ">" : `> ${line.trimEnd()}`))
    .join("\n");
}

/**
 * The text copied by "Copy with reference":
 *
 *     @docs/specs/live-feed.md (section "Event model")
 *     > selected text
 */
export function formatQuote(opts: {
  template: string;
  path: string;
  section: string | null;
  text: string;
  /** Localized wording for the section, e.g. `section "{name}"`. */
  sectionLabel: (name: string) => string;
}): string {
  const head = formatReference(opts.template, opts.path);
  const section = opts.section ? ` (${opts.sectionLabel(opts.section)})` : "";
  return `${head}${section}\n${quoteLines(opts.text)}`;
}

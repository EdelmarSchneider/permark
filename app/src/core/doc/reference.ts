// References: how "copy reference" and "copy with reference" write where a passage lives (pure).

export const DEFAULT_REFERENCE_TEMPLATE = "{path}#{section}";

export type Reference = {
  /** Project path of the document. */
  path: string;
  /** Anchor id of the nearest heading above the passage. */
  section: string | null;
  /** First line of the passage. */
  line: number | null;
};

function nameOf(path: string): string {
  return path.split("/").at(-1) ?? path;
}

/**
 * Fills the person's template: `{path} {section} {line} {name}`. An empty template is the default. A placeholder that
 * has no value takes its joiner with it (`#{section}`, `:{line}`), so a document-level reference stays clean.
 * Unknown `{names}` stay as written.
 */
export function formatReference(template: string, ref: Reference): string {
  const base = template.trim() === "" ? DEFAULT_REFERENCE_TEMPLATE : template;
  const values: Record<string, string | null> = {
    path: ref.path,
    section: ref.section,
    line: ref.line === null ? null : String(ref.line),
    name: nameOf(ref.path),
  };
  let text = base;
  for (const [name, value] of Object.entries(values)) {
    if (value !== null && value !== "") continue;
    text = text
      .split(`#{${name}}`)
      .join("")
      .split(`:{${name}}`)
      .join("")
      .split(`{${name}}`)
      .join("");
  }
  // one pass, so a value that looks like a placeholder is never filled in again
  text = text.replace(/\{(path|section|line|name)\}/g, (whole: string, name: string) => {
    const value = values[name];
    return value === undefined || value === null ? whole : value;
  });
  return text;
}

/** The selection as quote lines; blank lines stay as an empty quote line. */
export function quoteLines(text: string): string {
  return text
    .replace(/\r\n?/g, "\n")
    .trim()
    .split("\n")
    .map((line) => (line.trim() === "" ? ">" : `> ${line.trimEnd()}`))
    .join("\n");
}

/** "Copy with reference": the reference line, then the selection quoted. */
export function formatQuote(template: string, ref: Reference, selection: string): string {
  return `${formatReference(template, ref)}\n${quoteLines(selection)}`;
}

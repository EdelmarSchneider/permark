// Loose files: files opened in the Notes window that are not in the notes folder. The window refers to one by a path of its own,
// `@file/<id>/<file name>`, which holds an id and never a place of the disk (docs/design/v0.5.md section 5).

const PREFIX = "@file/";

/** Whether a path of the Notes window is one of a loose file (the file itself or something next to it). */
export function isLoosePath(path: string | null | undefined): boolean {
  return typeof path === "string" && path.startsWith(PREFIX);
}

/** Whether a loose file is plain text (`.txt`): it is edited in Source only, since Visual is for Markdown. */
export function isPlainTextName(path: string): boolean {
  return /\.(txt|text)$/i.test(path);
}

/** The name of a loose file, from its path for the window. */
export function looseName(path: string): string {
  const rest = path.startsWith(PREFIX) ? path.slice(PREFIX.length) : path;
  const at = rest.indexOf("/");
  return at < 0 ? rest : rest.slice(at + 1);
}

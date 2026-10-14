// Pure path helpers. Paths across the boundary are vault-relative, "/"-separated; worktree paths are
// prefixed with "@wt/<name>/".

export const WT_PREFIX = "@wt/";

export function basename(path: string): string {
  const i = path.lastIndexOf("/");
  return i < 0 ? path : path.slice(i + 1);
}

/** Parent folder of a path; "" for a top-level entry. A worktree root's parent is "". */
export function dirname(path: string): string {
  const i = path.lastIndexOf("/");
  return i < 0 ? "" : path.slice(0, i);
}

export function extname(path: string): string {
  const name = basename(path);
  const i = name.lastIndexOf(".");
  return i <= 0 ? "" : name.slice(i + 1).toLowerCase();
}

const DOC_EXTENSIONS = new Set(["md", "markdown", "mdx"]);

/** Markdown documents (the feed's "Docs only" filter, Follow and the search index care about these). */
export function isMarkdown(path: string): boolean {
  return DOC_EXTENSIONS.has(extname(path));
}

/** "@wt/feature-ui/docs/a.md" -> { worktree: "feature-ui", rel: "docs/a.md" }. */
export function splitWorktree(path: string): { worktree: string | null; rel: string } {
  if (!path.startsWith(WT_PREFIX)) return { worktree: null, rel: path };
  const rest = path.slice(WT_PREFIX.length);
  const i = rest.indexOf("/");
  return i < 0
    ? { worktree: rest, rel: "" }
    : { worktree: rest.slice(0, i), rel: rest.slice(i + 1) };
}

/** The folder shown next to a file name, without the worktree prefix. "" means the project root. */
export function folderLabel(path: string): string {
  return dirname(splitWorktree(path).rel);
}

/** A URL fragment as the reader wrote it: "a%C3%A7%C3%B5es" -> "ações". Invalid escapes are left as they are. */
export function decodeFragment(fragment: string): string {
  try {
    return decodeURIComponent(fragment);
  } catch {
    return fragment;
  }
}

/** Folders that must be expanded to reveal `path` in the tree, outermost first. */
export function ancestors(path: string): string[] {
  const { worktree, rel } = splitWorktree(path);
  const out: string[] = [];
  const base = worktree === null ? "" : `${WT_PREFIX}${worktree}`;
  if (worktree !== null) out.push(base);
  const segs = rel.split("/").filter(Boolean);
  let acc = base;
  for (const seg of segs.slice(0, -1)) {
    acc = acc === "" ? seg : `${acc}/${seg}`;
    out.push(acc);
  }
  return out;
}

export type Crumb = { name: string; path: string };

/** Breadcrumb segments for a document: the folders (clickable) and the file itself (last). */
export function breadcrumbs(path: string): Crumb[] {
  const { worktree, rel } = splitWorktree(path);
  const crumbs: Crumb[] = [];
  let acc = "";
  if (worktree !== null) {
    acc = `${WT_PREFIX}${worktree}`;
    crumbs.push({ name: `@wt ${worktree}`, path: acc });
  }
  for (const seg of rel.split("/").filter(Boolean)) {
    acc = acc === "" ? seg : `${acc}/${seg}`;
    crumbs.push({ name: seg, path: acc });
  }
  return crumbs;
}

const SCHEME = /^[a-z][a-z0-9+.-]+:/i; // two or more chars: "C:" is a drive letter, not a scheme

/**
 * Resolves a relative reference (an image `src`, for example) against the document that contains it.
 * Returns a vault-relative path, or null for URLs, drive paths and references that escape the root.
 */
export function resolveRelative(fromPath: string, ref: string): string | null {
  let target = ref.trim();
  if (target === "" || target.startsWith("//") || SCHEME.test(target)) return null;
  if (/^[a-z]:[\\/]/i.test(target)) return null;
  target = target.replace(/[?#].*$/, "").replace(/\\/g, "/");
  try {
    target = decodeURIComponent(target);
  } catch {
    // keep the raw text when it is not valid percent-encoding
  }
  const { worktree, rel } = splitWorktree(fromPath);
  const prefix = worktree === null ? "" : `${WT_PREFIX}${worktree}/`;
  const stack = target.startsWith("/") ? [] : dirname(rel).split("/").filter(Boolean);
  for (const seg of target.split("/")) {
    if (seg === "" || seg === ".") continue;
    if (seg === "..") {
      if (stack.length === 0) return null;
      stack.pop();
    } else {
      stack.push(seg);
    }
  }
  return stack.length === 0 ? null : prefix + stack.join("/");
}

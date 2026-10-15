// Comments as lists (docs/design/v0.4.md section 3): what the Comments tab groups and what the margin shows. Pure; the
// store keeps the comments and the components draw them.

import type { Comment } from "./types";

export type CommentGroup = { path: string; comments: Comment[] };

/** The longest quote a row shows, in characters. */
export const EXCERPT_CHARS = 140;

const byLine = (a: Comment, b: Comment): number =>
  a.line - b.line || a.createdMs - b.createdMs || (a.id < b.id ? -1 : a.id > b.id ? 1 : 0);

const byPath = (a: string, b: string): number => {
  const [x, y] = [a.toLowerCase(), b.toLowerCase()];
  return x < y ? -1 : x > y ? 1 : a < b ? -1 : a > b ? 1 : 0;
};

/** The comments still open, grouped by document: documents in path order, each document's comments by line. */
export function openByDocument(comments: readonly Comment[]): CommentGroup[] {
  const groups = new Map<string, Comment[]>();
  for (const comment of comments) {
    if (comment.done) continue;
    const list = groups.get(comment.path);
    if (list) list.push(comment);
    else groups.set(comment.path, [comment]);
  }
  return [...groups]
    .sort(([a], [b]) => byPath(a, b))
    .map(([path, list]) => ({ path, comments: list.sort(byLine) }));
}

/** The comments marked done, the latest first. */
export function doneComments(comments: readonly Comment[]): Comment[] {
  return comments
    .filter((comment) => comment.done)
    .sort(
      (a, b) =>
        (b.doneMs ?? b.createdMs) - (a.doneMs ?? a.createdMs) ||
        byPath(a.path, b.path) ||
        byLine(a, b),
    );
}

/** How many comments are open. */
export function openCount(comments: readonly Comment[]): number {
  return comments.reduce((count, comment) => count + (comment.done ? 0 : 1), 0);
}

/** The comments that get a bar in the margin of a document: open ones whose passage is where the comment says. */
export function barComments(comments: readonly Comment[], path: string | null): Comment[] {
  if (path === null) return [];
  return comments.filter((n) => n.path === path && !n.done && n.state === "anchored").sort(byLine);
}

/** Whether a document has comments that are not done (the file lists and tabs can say so). */
export function hasOpenComments(comments: readonly Comment[], path: string): boolean {
  return comments.some((comment) => comment.path === path && !comment.done);
}

/** A quote shortened to fit a row: spaces collapsed, cut at a word where it can be, with an ellipsis. */
export function excerpt(text: string, max = EXCERPT_CHARS): string {
  const flat = text.replace(/\s+/g, " ").trim();
  if (flat.length <= max) return flat;
  const cut = flat.slice(0, max);
  const space = cut.lastIndexOf(" ");
  return `${(space > max * 0.6 ? cut.slice(0, space) : cut).trimEnd()}…`;
}

/** The headings above a passage as a trail ("Plan › Rollout"). */
export function trail(headingPath: readonly string[]): string {
  return headingPath.join(" › ");
}

/** The document a comment is on, as a group title: its file name, and the folder it is in. */
export function documentTitle(path: string): { name: string; folder: string } {
  const slash = path.lastIndexOf("/");
  return slash < 0
    ? { name: path, folder: "" }
    : { name: path.slice(slash + 1), folder: path.slice(0, slash) };
}

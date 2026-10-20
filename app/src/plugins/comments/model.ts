// Comments as lists and marks (pure): what the Comments tab groups, which blocks of the page carry a bar, where a
// selection sits (the headings above it, its lines), and the prompt that hands comments to an agent.

import type { Block, CommentView, OutlineItem } from "../../api";

export type CommentGroup = { path: string; comments: CommentView[] };

/** The longest quote a row shows, in characters. */
export const EXCERPT_CHARS = 140;
/** How much of a passage a prompt quotes, in characters. */
export const QUOTE_CHARS = 800;

const byLine = (a: CommentView, b: CommentView): number =>
  a.line - b.line || a.createdMs - b.createdMs || (a.id < b.id ? -1 : a.id > b.id ? 1 : 0);

const byPath = (a: string, b: string): number => {
  const [x, y] = [a.toLowerCase(), b.toLowerCase()];
  return x < y ? -1 : x > y ? 1 : a < b ? -1 : a > b ? 1 : 0;
};

/** The comments still open, grouped by document: documents in path order, each document's comments by line. */
export function openByDocument(comments: readonly CommentView[]): CommentGroup[] {
  const groups = new Map<string, CommentView[]>();
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
export function doneComments(comments: readonly CommentView[]): CommentView[] {
  return comments
    .filter((comment) => comment.done)
    .sort(
      (a, b) =>
        (b.doneMs ?? b.createdMs) - (a.doneMs ?? a.createdMs) ||
        byPath(a.path, b.path) ||
        byLine(a, b),
    );
}

/** How many comments each document has open. */
export function openCounts(comments: readonly CommentView[]): ReadonlyMap<string, number> {
  const counts = new Map<string, number>();
  for (const comment of comments) {
    if (!comment.done) counts.set(comment.path, (counts.get(comment.path) ?? 0) + 1);
  }
  return counts;
}

/** The comments that get a bar in the margin of a document: open ones whose passage is where the comment says. */
export function barComments(comments: readonly CommentView[], path: string | null): CommentView[] {
  if (path === null) return [];
  return comments
    .filter((c) => c.path === path && !c.done && (c.state === "anchored" || c.state === "moved"))
    .sort(byLine);
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

// ---- the page --------------------------------------------------------------------------------------------------

/** The blocks of the page that a passage between two lines touches (a block holds the lines it spans). */
export function touchedBlocks(
  blocks: readonly Pick<Block, "id" | "lineStart" | "lineEnd">[],
  line: number,
  endLine: number,
): string[] {
  const first = Math.min(line, endLine);
  const last = Math.max(line, endLine);
  return blocks
    .filter((b) => b.lineStart > 0 && b.lineStart <= last && b.lineEnd >= first)
    .map((b) => b.id);
}

/** The headings above a block (its own heading last when it is one): the labels of the outline, by level. */
export function headingPathAt(
  blocks: readonly Pick<Block, "id">[],
  outline: readonly OutlineItem[],
  blockId: string,
): string[] {
  const at = blocks.findIndex((b) => b.id === blockId);
  if (at < 0) return [];
  const stack: { level: number; label: string }[] = [];
  for (const item of outline) {
    const itemAt = blocks.findIndex((b) => b.id === item.block);
    if (itemAt < 0 || itemAt > at) continue;
    while (stack.length > 0 && (stack[stack.length - 1]?.level ?? 0) >= item.level) stack.pop();
    stack.push({ level: item.level, label: item.label });
  }
  return stack.map((s) => s.label);
}

// ---- the prompt for an agent -----------------------------------------------------------------------------------

/** Where a comment sits, as one phrase: its section (the headings above it) and its line. */
function place(comment: CommentView): string {
  return [trail(comment.headingPath), `line ${comment.line}`]
    .filter((part) => part !== "")
    .join(", ");
}

/**
 * One comment as a short Markdown prompt, ready to paste into an agent's chat. English whatever the app's language:
 * it is written for an agent, not for the person.
 */
export function agentCommentPrompt(comment: CommentView): string {
  const where = place(comment);
  return [
    `Please address this comment on ${comment.path}${where ? ` (${where})` : ""}:`,
    "",
    `> ${excerpt(comment.quote, QUOTE_CHARS)}`,
    "",
    comment.text,
    "",
    "When you have addressed it, mark it done with `resolve_comment` and a short reply.",
  ].join("\n");
}

/** The open comments as one Markdown prompt, a section per document. Nothing when there is nothing to send. */
export function agentCommentsPrompt(groups: readonly CommentGroup[]): string {
  if (groups.length === 0) return "";
  const sections = groups.map((group) =>
    [
      `## ${group.path}`,
      "",
      group.comments
        .map((comment, index) =>
          [
            `${index + 1}. ${place(comment)}:`,
            "",
            `   > ${excerpt(comment.quote, QUOTE_CHARS)}`,
            "",
            `   ${comment.text}`,
          ].join("\n"),
        )
        .join("\n\n"),
      "",
    ].join("\n"),
  );
  return [
    "Please go through these comments on the documents:",
    "",
    sections.join("\n"),
    "When you have addressed a comment, mark it done with `resolve_comment` and a short reply.",
  ].join("\n");
}

// Prompts that hand comments to an agent (docs/design/v0.4.md section 3): the ready-to-paste text the "Send to agent"
// actions put on the clipboard. The store copies it, the components only ask for it. The wording is English on purpose
// whatever the app's language: it is written for an agent, not for the person.

import { excerpt, trail, type CommentGroup } from "./comments";
import type { Comment } from "./types";

/** How much of a passage a prompt quotes, in characters. A prompt can carry far more than a row. */
export const QUOTE_CHARS = 800;

/** Where a comment sits, as one phrase: its section (the headings above it) and its line. */
function place(comment: Comment): string {
  return [trail(comment.headingPath), `line ${comment.line}`]
    .filter((part) => part !== "")
    .join(", ");
}

/** One comment as a short Markdown prompt, ready to paste into an agent's chat. */
export function agentCommentPrompt(comment: Comment): string {
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

/**
 * The open comments as one Markdown prompt: a section per document, in the order and grouping the caller gives them
 * (`openByDocument()` sorts both). Nothing when there is nothing to send.
 */
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

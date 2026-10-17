import { describe, expect, it } from "vitest";
import { agentCommentPrompt, agentCommentsPrompt, QUOTE_CHARS } from "./agentPrompt";
import { openByDocument } from "./comments";
import type { Comment } from "./types";

let counter = 0;
const comment = (patch: Partial<Comment> = {}): Comment => ({
  id: `n${++counter}`,
  path: "docs/plan.md",
  text: "a comment",
  quote: "a passage",
  headingPath: ["Plan"],
  line: 1,
  endLine: 1,
  createdMs: 1000 + counter,
  state: "anchored",
  done: false,
  ...patch,
});

describe("agentCommentPrompt", () => {
  it("writes one comment as a prompt that names the document, the section, the line and the words", () => {
    const text = agentCommentPrompt(
      comment({
        headingPath: ["Plan", "Rollout"],
        line: 40,
        quote: "ship it on Friday",
        text: "simplify this",
      }),
    );
    expect(text).toContain("docs/plan.md");
    expect(text).toContain("Plan › Rollout");
    expect(text).toContain("line 40");
    expect(text).toContain("> ship it on Friday");
    expect(text).toContain("simplify this");
    expect(text).toContain("mark it done with `resolve_comment`");
  });

  it("names the document and the line alone when the passage has no section", () => {
    expect(agentCommentPrompt(comment({ headingPath: [] }))).toContain("docs/plan.md (line 1)");
  });

  it("collapses the whitespace of the passage but keeps far more of it than a row does", () => {
    expect(agentCommentPrompt(comment({ quote: "  two  \n\n lines " }))).toContain("> two lines");
  });

  it("cuts a passage that goes on and on, saying so", () => {
    const text = agentCommentPrompt(comment({ quote: "x".repeat(QUOTE_CHARS + 100) }));
    expect(text).toContain(`${"x".repeat(QUOTE_CHARS)}…`);
  });
});

describe("agentCommentsPrompt", () => {
  it("gives each document a section and numbers its comments by line", () => {
    const text = agentCommentsPrompt(
      openByDocument([
        comment({ path: "docs/b.md", line: 4, text: "second doc" }),
        comment({ path: "a.md", line: 9 }),
        comment({ path: "docs/b.md", line: 2 }),
      ]),
    );
    expect(text.indexOf("## a.md")).toBeGreaterThan(-1);
    const bAt = text.indexOf("## docs/b.md");
    expect(bAt).toBeGreaterThan(text.indexOf("## a.md"));
    expect(text.slice(bAt)).toContain("1. Plan, line 2");
    expect(text.slice(bAt)).toContain("2. Plan, line 4");
    expect(text).toContain("second doc");
  });

  it("asks the agent to mark each comment done when it has addressed it", () => {
    expect(agentCommentsPrompt(openByDocument([comment()]))).toContain(
      "When you have addressed a comment, mark it done with `resolve_comment`.",
    );
  });

  it("leaves out the comments that are done, because the caller groups the open ones", () => {
    const text = agentCommentsPrompt(
      openByDocument([
        comment({ done: true, text: "already settled" }),
        comment({ text: "still open" }),
      ]),
    );
    expect(text).toContain("still open");
    expect(text).not.toContain("already settled");
  });

  it("always writes the same text, whatever order the comments arrived in", () => {
    const one = [comment(), comment({ path: "docs/b.md" })];
    const two = [comment({ path: "docs/b.md" }), comment()];
    expect(agentCommentsPrompt(openByDocument(two))).toBe(agentCommentsPrompt(openByDocument(one)));
  });

  it("says nothing when there is nothing to send", () => {
    expect(agentCommentsPrompt([])).toBe("");
    expect(agentCommentsPrompt(openByDocument([comment({ done: true })]))).toBe("");
  });
});

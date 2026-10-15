// Comments for the mock backend: kept in memory, announced the way the real backend does. The passage of a comment
// is taken as found (the mock has no documents to look it up in); the mock controls can play an agent that resolves one.

import { emitMock } from "../api";
import type { NewComment, Comment } from "../types";

let comments: Comment[] = [];
let counter = 0;

function announce(): void {
  setTimeout(() => emitMock("comments-changed", null), 0);
}

const byDocument = (a: Comment, b: Comment): number =>
  a.path < b.path ? -1 : a.path > b.path ? 1 : a.line - b.line || a.createdMs - b.createdMs;

export function listComments(): Comment[] {
  return comments.map((comment) => ({ ...comment })).sort(byDocument);
}

export function addComment(draft: NewComment): Comment {
  if (draft.quote.trim() === "" || draft.text.trim() === "") {
    throw { kind: "emptyComment", message: "a comment needs a selected passage and some text" };
  }
  counter++;
  const line = Math.max(1, draft.line);
  const comment: Comment = {
    id: `n${counter.toString(16).padStart(8, "0")}`,
    path: draft.path,
    text: draft.text.trim(),
    quote: draft.quote.trim(),
    headingPath: draft.headingPath,
    line,
    endLine: Math.max(line, draft.endLine),
    createdMs: Date.now(),
    state: "anchored",
    done: false,
  };
  comments.push(comment);
  announce();
  return { ...comment };
}

function find(id: string): Comment {
  const comment = comments.find((candidate) => candidate.id === id);
  if (!comment)
    throw {
      kind: "commentNotFound",
      message: `there is no comment with the id ${id}`,
      subject: id,
    };
  return comment;
}

export function setCommentDone(id: string, done: boolean): Comment {
  const comment = find(id);
  if (done && !comment.done) {
    Object.assign(comment, { done: true, doneMs: Date.now(), doneBy: "person" as const });
  } else if (!done && comment.done) {
    comment.done = false;
    delete comment.doneMs;
    delete comment.doneBy;
    delete comment.reply;
  }
  announce();
  return { ...comment };
}

export function deleteComment(id: string): void {
  find(id);
  comments = comments.filter((comment) => comment.id !== id);
  announce();
}

/** Plays an agent that resolves a comment (the oldest open one) and leaves a reply. */
export function agentResolves(reply: string): void {
  const comment = comments.find((candidate) => !candidate.done);
  if (!comment) return;
  Object.assign(comment, { done: true, doneMs: Date.now(), doneBy: "agent" as const, reply });
  announce();
}

/** Forgets every comment (a new mock project). */
export function resetComments(): void {
  comments = [];
}

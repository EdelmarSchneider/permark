// Review notes for the mock backend: kept in memory, announced the way the real backend does. The passage of a note
// is taken as found (the mock has no documents to look it up in); the mock controls can play an agent that resolves one.

import { emitMock } from "../api";
import type { NewNote, Note } from "../types";

let notes: Note[] = [];
let counter = 0;

function announce(): void {
  setTimeout(() => emitMock("notes-changed", null), 0);
}

const byDocument = (a: Note, b: Note): number =>
  a.path < b.path ? -1 : a.path > b.path ? 1 : a.line - b.line || a.createdMs - b.createdMs;

export function listNotes(): Note[] {
  return notes.map((note) => ({ ...note })).sort(byDocument);
}

export function addNote(draft: NewNote): Note {
  if (draft.quote.trim() === "" || draft.text.trim() === "") {
    throw { kind: "emptyNote", message: "a note needs a selected passage and some text" };
  }
  counter++;
  const line = Math.max(1, draft.line);
  const note: Note = {
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
  notes.push(note);
  announce();
  return { ...note };
}

function find(id: string): Note {
  const note = notes.find((candidate) => candidate.id === id);
  if (!note)
    throw { kind: "noteNotFound", message: `there is no note with the id ${id}`, subject: id };
  return note;
}

export function setNoteDone(id: string, done: boolean): Note {
  const note = find(id);
  if (done && !note.done) {
    Object.assign(note, { done: true, doneMs: Date.now(), doneBy: "person" as const });
  } else if (!done && note.done) {
    note.done = false;
    delete note.doneMs;
    delete note.doneBy;
    delete note.reply;
  }
  announce();
  return { ...note };
}

export function deleteNote(id: string): void {
  find(id);
  notes = notes.filter((note) => note.id !== id);
  announce();
}

/** Plays an agent that resolves a note (the oldest open one) and leaves a reply. */
export function agentResolves(reply: string): void {
  const note = notes.find((candidate) => !candidate.done);
  if (!note) return;
  Object.assign(note, { done: true, doneMs: Date.now(), doneBy: "agent" as const, reply });
  announce();
}

/** Forgets every note (a new mock project). */
export function resetNotes(): void {
  notes = [];
}

// What a note's text says about it, for the places that cannot wait for the backend's list: the title in the header and
// the word count in the status bar, while the person types. The list itself shows what the backend made of the first lines
// (`title`, `snippet` of a `NoteEntry`); this is the same idea, a little simpler.

/** The title of a note: its first line with the Markdown marks taken off. Empty for a note with no text. */
export function noteTitle(text: string): string {
  let body = text.replace(/^﻿/, "");
  // front matter says nothing about the note
  const front = /^---\r?\n[\s\S]*?\r?\n(?:---|\.\.\.)[ \t]*(?:\r?\n|$)/.exec(body);
  if (front) body = body.slice(front[0].length);
  for (const raw of body.split(/\r?\n/)) {
    const line = cleanLine(raw);
    if (line !== "") return line.slice(0, 120);
  }
  return "";
}

/** One line without its marks; empty when nothing is left (a blank line, a rule, a code fence). */
function cleanLine(raw: string): string {
  const line = raw.trim();
  if (line === "" || /^(```|~~~)/.test(line)) return "";
  if (/^[-*_=|:\s]{3,}$/.test(line) && /[-*_=]{3}/.test(line.replace(/[\s|:]/g, ""))) return "";
  let rest = line;
  for (let round = 0; round < 6; round++) {
    const before = rest;
    rest = rest
      .replace(/^>\s?/, "")
      .replace(/^#{1,6}(?=\s|$)/, "")
      .replace(/^[-*+]\s+/, "")
      .replace(/^\d+[.)]\s+/, "")
      .replace(/^\[[ xX]\]\s+/, "")
      .trimStart();
    if (rest === before) break;
  }
  return rest
    .replace(/!?\[([^\]]*)\]\([^)]*\)/g, "$1")
    .replace(/<(https?:\/\/[^>\s]+)>/g, "$1")
    .replace(/<\/?[a-zA-Z][^>]*>/g, "")
    .replace(/\\([!-/:-@[-`{-~])/g, "$1")
    .replace(/[`*~]+/g, "")
    .replace(/(^|[^\p{L}\p{N}])_+|_+(?=[^\p{L}\p{N}]|$)/gu, "$1")
    .replace(/\s+/g, " ")
    .trim();
}

/** How many words a note has: runs of characters between blanks that have a letter or a digit (not `-`, `#` or `---`). */
export function countWords(text: string): number {
  let count = 0;
  for (const run of text.split(/\s+/)) {
    if (/[\p{L}\p{N}]/u.test(run)) count += 1;
  }
  return count;
}

/** Whether the text has nothing but blank characters (a byte order mark counts as blank). */
export function isBlank(text: string): boolean {
  return text.replace(/﻿/g, "").trim() === "";
}

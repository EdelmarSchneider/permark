// The earlier versions of the mock project's files (`review.versions`) and what the review shows against each. Every
// committed file shares one short history; one document has a long one (pages), one was never committed, and a
// picture is not text. Canned: nothing here compares texts.

import type { Block } from "./generated/Block";
import { categoryOfName } from "./mockFs";
import type { ReviewChange, ReviewVersions, VersionRow } from "./types";

const HOUR = 3_600_000;
const DAY = 24 * HOUR;
/** Rows in a page, as the real list. */
const PAGE = 50;

/** The commit the mock repository is at. */
export const MOCK_HEAD = "a1b2c3d4e5f60718293a4b5c6d7e8f9012345678";
/** A document with more versions than a page holds, in a copy of the repository without its whole history: the list
 *  says older versions may be missing, and the oldest one listed is not on this computer. */
export const MOCK_LONG_HISTORY = "docs/handbook.md";
const LONG_ROWS = 120;
/** The path a document had before its first version here (the list names it). */
const RENAMED: Readonly<Record<string, string>> = { "docs/guide.md": "docs/manual.md" };

/** How a file stands toward the last commit. */
export type MockFileState = "untracked" | "changed" | "unchanged";

type Commit = Omit<VersionRow, "atMs" | "same"> & { ago: number };

/** Newest first. The last one added the file. */
const COMMITS: readonly Commit[] = [
  {
    commit: MOCK_HEAD,
    ago: 2 * HOUR,
    author: "Ana Souza",
    coAuthors: ["Rafael Lima"],
    subject: "Rewrite the install steps",
  },
  {
    commit: "7c9e21f0a4b83d5566e1f2a39b0c4d7e8f1a2b3c",
    ago: 26 * HOUR,
    author: "Ana Souza",
    coAuthors: [],
    subject: "Fix typos",
  },
  {
    commit: "3f5a9c1e7b2d4068a1c3e5f7092b4d6e8a0c2e4f",
    ago: 9 * DAY,
    author: "Lin Chen",
    coAuthors: [],
    subject:
      "Restructure the page into install, first steps and troubleshooting so that new readers find the setup notes without searching all of it",
  },
  {
    commit: "0d4c8b2a6e1f3975c0b2a4d6e8f1a3c5b7d9e0f2",
    ago: 96 * DAY,
    author: "Marco Rossi",
    coAuthors: [],
    subject: "Add the first draft",
  },
];

/** What the three newer commits replaced, newest first: the text a version before them still has. */
const PASSAGES: readonly {
  kind: ReviewChange["kind"];
  start: number;
  len: number;
  text: string;
}[] = [
  { kind: "edited", start: 1, len: 1, text: "Install the app, then open a folder." },
  { kind: "removed", start: 2, len: 0, text: "This note is for the old installer." },
  { kind: "added", start: 0, len: 1, text: "" },
];

function commitsOf(path: string): readonly Commit[] {
  if (path !== MOCK_LONG_HISTORY) return COMMITS;
  const first = COMMITS.at(-1);
  const filler = Array.from({ length: LONG_ROWS - COMMITS.length }, (_, i): Commit => {
    return {
      commit: (i + 1).toString(16).padStart(40, "e"),
      ago: 10 * DAY + i * 12 * HOUR,
      author: i % 3 === 0 ? "Lin Chen" : "Ana Souza",
      coAuthors: [],
      subject: `Update the handbook (${LONG_ROWS - COMMITS.length - i})`,
    };
  });
  return [...COMMITS.slice(0, -1), ...filler, ...(first ? [first] : [])];
}

function isText(path: string): boolean {
  const category = categoryOfName(path.split("/").at(-1) ?? path);
  return !["image", "audio", "video", "office"].includes(category);
}

/** One page of the versions of `path`, or why it has none. `after`: where the page before ended. */
export function mockVersions(path: string, after: number, file: MockFileState): ReviewVersions {
  if (!isText(path)) return { state: "unavailable", reason: "unsupported" };
  if (file === "untracked") return { state: "unavailable", reason: "notCommitted" };
  const commits = commitsOf(path);
  const end = Math.min(after + PAGE, commits.length);
  const now = Date.now();
  return {
    state: "list",
    rows: commits.slice(after, end).map(({ ago, ...row }, i) => ({
      ...row,
      atMs: now - ago,
      // the newest version is what the last commit holds: the text now, unless the file changed since
      same: after + i === 0 && file === "unchanged",
    })),
    next: end < commits.length ? end : null,
    renamedFrom: end < commits.length ? null : (RENAMED[path] ?? null),
    cut: path === MOCK_LONG_HISTORY,
  };
}

function ghost(id: string, text: string): Block {
  return { id, kind: "p", lineStart: 1, lineEnd: 1, html: `<p data-b="${id}">${text}</p>` };
}

/** What the commits after `commit` changed in `path` (none for the newest one), as the review draws it; `null` when
 *  the version cannot be had: no version of this file, or not in a copy without its whole history. */
export function mockVersionChanges(
  path: string,
  commit: string,
  unit: "blocks" | "lines",
): ReviewChange[] | null {
  const commits = commitsOf(path);
  const at = commits.findIndex((row) => row.commit === commit);
  if (at < 0 || !isText(path)) return null;
  if (path === MOCK_LONG_HISTORY && at === commits.length - 1) return null;
  return PASSAGES.slice(0, at).map((passage, index) => ({
    index,
    kind: passage.kind,
    start: passage.start,
    len: passage.len,
    ghosts: unit === "blocks" && passage.text !== "" ? [ghost(`old-${index}`, passage.text)] : [],
    old: unit === "lines" && passage.text !== "" ? [passage.text] : [],
    whitespace: false,
  }));
}

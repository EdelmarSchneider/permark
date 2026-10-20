// Search over the in-memory project (dev:mock, tests): the same shapes as the backend, none of its ranking rules (the
// Rust side has those, with their tests). Words must all appear in a section; a path matches as a subsequence.

import { categoryOfName, type MockProject } from "./mockFs";
import type { FileHit, FolderHit, PathResults, Recent, SectionHit, SectionResults } from "./types";

const fold = (s: string): string => s.normalize("NFD").replace(/\p{M}/gu, "").toLowerCase();

type Section = { path: string; headingPath: string[]; line: number; text: string };

function sectionsOf(path: string, text: string): Section[] {
  const out: Section[] = [];
  const stack: { level: number; label: string }[] = [];
  let current: Section = { path, headingPath: [], line: 1, text: "" };
  let started = false;
  const lines = text.split("\n");
  lines.forEach((line, at) => {
    const heading = /^(#{1,6})\s+(.*)$/.exec(line);
    if (heading?.[1] !== undefined && heading[2] !== undefined) {
      if (started || current.text.trim() !== "") out.push(current);
      const level = heading[1].length;
      while (stack.length > 0 && (stack[stack.length - 1]?.level ?? 0) >= level) stack.pop();
      stack.push({ level, label: heading[2].trim() });
      current = { path, headingPath: stack.map((h) => h.label), line: at + 1, text: "" };
      started = true;
    } else current.text += `${line} `;
  });
  if (started || current.text.trim() !== "") out.push(current);
  return out;
}

function anchorOf(label: string): string {
  return fold(label)
    .replace(/[^\p{L}\p{N}\s-]/gu, "")
    .trim()
    .replace(/\s+/g, "-");
}

/** Which paths a search may answer with (tags and silence are the plugins' business). */
export type Allows = (path: string) => boolean;
const ANY: Allows = () => true;

export function mockSections(
  project: MockProject,
  query: string,
  limit: number,
  allows: Allows = ANY,
): SectionResults {
  const words = fold(query)
    .split(/\s+/)
    .filter((w) => w.length > 1);
  if (words.length === 0) return { hits: [], partial: false };
  const hits: SectionHit[] = [];
  for (const [path, text] of project.files) {
    if (!/\.(md|markdown|txt)$/i.test(path) || !allows(path)) continue;
    for (const section of sectionsOf(path, text)) {
      const heading = fold(section.headingPath.join(" "));
      const body = fold(`${section.text} ${path}`);
      if (!words.every((w) => heading.includes(w) || body.includes(w))) continue;
      const score = words.reduce(
        (sum, w) => sum + (heading.includes(w) ? 2 : 0) + (body.includes(w) ? 1 : 0),
        0,
      );
      const plain = section.text.trim().replace(/\s+/g, " ");
      const first = words.map((w) => fold(plain).indexOf(w)).find((i) => i >= 0) ?? -1;
      const snippet =
        first < 0
          ? [{ text: section.headingPath.at(-1) ?? plain.slice(0, 80), mark: false }]
          : [
              { text: plain.slice(Math.max(0, first - 20), first), mark: false },
              { text: plain.slice(first, first + (words[0]?.length ?? 1)), mark: true },
              { text: plain.slice(first + (words[0]?.length ?? 1), first + 100), mark: false },
            ].filter((p) => p.text !== "");
      const last = section.headingPath.at(-1);
      hits.push({
        path,
        headingPath: section.headingPath,
        anchor: last === undefined ? null : anchorOf(last),
        line: section.line,
        snippet,
        score,
      });
    }
  }
  hits.sort((a, b) => b.score - a.score || a.path.localeCompare(b.path) || a.line - b.line);
  return { hits: hits.slice(0, limit), partial: false };
}

/** Indices of the characters of `q` found in order in `path` (a substring when there is one). */
function positionsOf(q: string, path: string): number[] | null {
  const hay = fold(path);
  const at = hay.indexOf(q);
  if (at >= 0) return Array.from({ length: q.length }, (_, i) => at + i);
  const found: number[] = [];
  let from = 0;
  for (const ch of q) {
    const i = hay.indexOf(ch, from);
    if (i < 0) return null;
    found.push(i);
    from = i + 1;
  }
  return found;
}

/** A dot file or a file inside a dot folder (the worktree prefix `@wt/<name>/` not counted). */
export function hiddenPath(path: string): boolean {
  const own = path.startsWith("@wt/") ? path.split("/").slice(2).join("/") : path;
  return own.split("/").some((segment) => segment.startsWith("."));
}

export function mockFiles(
  project: MockProject,
  query: string,
  limit: number,
  folderLimit: number,
  allows: Allows = ANY,
  hiddenLast = false,
): PathResults {
  const q = fold(query.trim());
  if (q === "") return { files: [], folders: [], partial: false };
  const files: (FileHit & { rank: number })[] = [];
  for (const path of project.files.keys()) {
    const positions = positionsOf(q, path);
    if (positions === null || !allows(path)) continue;
    const name = path.slice(path.lastIndexOf("/") + 1);
    const rank = fold(name).startsWith(q) ? 0 : fold(name).includes(q) ? 1 : 2;
    files.push({ path, category: categoryOfName(name), positions, hidden: hiddenPath(path), rank });
  }
  // hidden paths second when the tree does not show them (as the backend)
  const last = (path: string): number => (hiddenLast && hiddenPath(path) ? 1 : 0);
  files.sort(
    (a, b) => last(a.path) - last(b.path) || a.rank - b.rank || a.path.length - b.path.length,
  );
  const folders: FolderHit[] = [];
  for (const path of project.dirs) {
    const positions = positionsOf(q, path);
    if (positions !== null) folders.push({ path, positions, hidden: hiddenPath(path) });
  }
  folders.sort((a, b) => last(a.path) - last(b.path) || a.path.length - b.path.length);
  return {
    files: files.slice(0, limit).map(({ rank: _rank, ...hit }) => hit),
    folders: folders.slice(0, folderLimit),
    partial: false,
  };
}

export function mockRecent(
  project: MockProject,
  opened: string[],
  limit: number,
  allows: Allows = ANY,
): Recent {
  const hit = (path: string): FileHit => ({
    path,
    category: categoryOfName(path.slice(path.lastIndexOf("/") + 1)),
    positions: [],
    hidden: hiddenPath(path),
  });
  return {
    opened: opened
      .filter((p) => project.files.has(p) && allows(p))
      .slice(0, limit)
      .map(hit),
    changed: [...project.files.keys()]
      .filter((p) => /\.(md|txt)$/.test(p) && allows(p))
      .sort()
      .slice(0, limit)
      .map(hit),
  };
}

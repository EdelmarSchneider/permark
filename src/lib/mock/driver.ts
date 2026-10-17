// Mock "backend activity" for VITE_MOCK: a timer that behaves like an agent working in the vault, and
// one-shot actions the mock controls panel can trigger. Everything goes through `emitMock`, exactly like
// the real events would arrive.

import { emitMock } from "../api";
import type { FsBatch, FsChange } from "../types";
import { deleteDocument, editDocument, mockGit, mockSnapshot, restoreDocument } from "./fixtures";
import { noteChanged, noteGone } from "./mockUnread";

const batch = (changes: FsChange[], extra: Partial<FsBatch> = {}): FsBatch => ({
  changes,
  bulk: null,
  rescan: false,
  ...extra,
});

const change = (kind: FsChange["kind"], path: string): FsChange => ({
  kind,
  path,
  atMs: Date.now(),
});

/** An agent saved a document. */
export function emitSave(path: string): void {
  editDocument(path);
  noteChanged(path);
  emitMock("fs-change", batch([change("modified", path)]));
}

/** Four documents saved within a couple of seconds (one toast that counts up). */
export function emitBurst(): void {
  const paths = [
    "docs/research/watchers.md",
    "docs/specs/watcher.md",
    "docs/specs/glossary.md",
    "@wt/feature-ui/docs/ui-notes.md",
  ];
  paths.forEach((path, i) => setTimeout(() => emitSave(path), i * 450));
}

/** A checkout: 143 files in one bulk batch (no toast, no Follow). */
export function emitBulk(): void {
  const docs = [
    "docs/specs/live-feed.md",
    "docs/specs/watcher.md",
    "docs/research/storage.md",
    "docs/world/lore.md",
    "docs/world/outline.md",
    "plan.md",
    "README.md",
    "CHANGELOG.md",
  ];
  const others = Array.from({ length: 70 }, (_, i) => `src/generated/file-${i}.rs`);
  const paths = [...docs, ...others];
  const changes = paths.map((path, i) => change(i % 7 === 0 ? "created" : "modified", path));
  for (const path of docs) {
    editDocument(path);
    noteChanged(path);
  }
  emitMock(
    "fs-change",
    batch(changes, { bulk: { count: 143, reason: "gitOperation", paths }, rescan: false }),
  );
}

export function emitDelete(path: string): void {
  deleteDocument(path);
  noteGone(path);
  emitMock("fs-change", batch([change("removed", path)]));
  emitMock("tree-changed", null);
}

export function emitRestore(path: string): void {
  restoreDocument(path);
  noteChanged(path);
  emitMock("fs-change", batch([change("created", path)]));
  emitMock("tree-changed", null);
}

export function setRepoState(state: "clean" | "rebase" | "merge"): void {
  mockGit.repoState = state;
  mockGit.generation += 1;
  emitMock("git-status-changed", mockSnapshot());
}

export function setGitError(on: boolean): void {
  mockGit.gitError = on
    ? "detected dubious ownership in repository at 'C:/Users/demo/permark-docs'"
    : null;
  mockGit.generation += 1;
  emitMock("git-status-changed", mockSnapshot());
}

function rampIndexing(): void {
  const total = 1280;
  let indexed = 0;
  emitMock("index-progress", { indexed, total });
  const timer = setInterval(() => {
    indexed = Math.min(total, indexed + 160);
    emitMock("index-progress", { indexed, total });
    if (indexed >= total) clearInterval(timer);
  }, 450);
}

/**
 * Starts the scripted activity. `quiet` only runs the indexing ramp (`?mock=quiet`), which keeps
 * screenshots and manual testing free of surprises.
 */
export function startMockDriver(quiet: boolean): () => void {
  const timers: ReturnType<typeof setTimeout>[] = [];
  const at = (ms: number, fn: () => void): void => {
    timers.push(setTimeout(fn, ms));
  };
  at(1200, rampIndexing);
  if (quiet) return () => timers.forEach(clearTimeout);

  at(9000, () => emitSave("docs/specs/live-feed.md"));
  at(12000, () => emitSave("docs/research/watchers.md"));
  at(15000, () => emitSave("@wt/feature-ui/docs/ui-notes.md"));
  at(24000, emitBulk);
  const loop = setInterval(() => {
    emitSave("docs/specs/live-feed.md");
    if (Math.random() < 0.4) emitSave("docs/specs/watcher.md");
  }, 22000);
  return () => {
    timers.forEach(clearTimeout);
    clearInterval(loop);
  };
}

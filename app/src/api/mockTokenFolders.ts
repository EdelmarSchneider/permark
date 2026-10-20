// What the token count plugin answers for a folder count in the mock backend: the same states in the same order as
// the real job (scanning, a question for a big folder, counting in steps, done | cancelled), over the files of the mock
// project, with a stand-in for each tokenizer. In memory.

import type { FolderBadges } from "./generated/FolderBadges";
import type { FolderJob } from "./generated/FolderJob";
import type { FolderSkip } from "./generated/FolderSkip";
import type { FolderSummary } from "./generated/FolderSummary";
import type { AppPluginEventName, AppPluginEvents, PluginMethods } from "./types";

export type FolderMethods = Pick<
  PluginMethods,
  | "token-count.folderStart"
  | "token-count.folderConfirm"
  | "token-count.folderCancel"
  | "token-count.folderDismiss"
  | "token-count.folderJob"
  | "token-count.folders"
  | "token-count.folderSummary"
>;

/** The sizes at which the guard acts (the real ones by default). */
export type Thresholds = {
  confirmFiles: number;
  confirmBytes: number;
  refuseFiles: number;
  refuseBytes: number;
  autoFiles: number;
  autoBytes: number;
};

export const STANDARD: Thresholds = {
  confirmFiles: 2_000,
  confirmBytes: 50_000_000,
  refuseFiles: 50_000,
  refuseBytes: 1_000_000_000,
  autoFiles: 200,
  autoBytes: 5_000_000,
};

export type FoldersProbe = {
  thresholds: Thresholds;
  /** Milliseconds between two steps of a count. */
  stepMs: number;
  /** Steps a count takes. */
  steps: number;
  /** Calls made: `folderStart <folder> <tokenizer> [auto]`, `folderCancel`, ... */
  calls: string[];
  /** Something under `path` changed: results above it are stale. */
  touch(path: string): void;
  /** Folders whose result is kept. */
  results(): string[];
  /** Back to the start: no job, no result, nothing counted before. */
  reset(): void;
};

const BINARY = new Set(["png", "jpg", "jpeg", "gif", "webp", "mp3", "mp4", "pdf", "docx", "xlsx"]);

function extOf(path: string): string {
  const name = path.slice(path.lastIndexOf("/") + 1);
  const dot = name.lastIndexOf(".");
  return dot <= 0 || dot + 1 === name.length ? "" : name.slice(dot + 1).toLowerCase();
}

function covers(folder: string, path: string): boolean {
  return folder === "" || path === folder || path.startsWith(`${folder}/`);
}

type Stored = { summary: FolderSummary; stale: boolean };

export function createMockFolders(
  emit: <E extends AppPluginEventName>(event: E, payload: AppPluginEvents[E]) => void,
  files: () => ReadonlyMap<string, string>,
  root: () => string,
  /** Counts a text by a tokenizer. */
  count: (tokenizer: string, text: string) => number,
  /** An exact tokenizer whose vocabulary is on this computer. */
  ready: (tokenizer: string) => boolean,
  exact: (tokenizer: string) => boolean,
) {
  let job: FolderJob = { state: "idle" };
  let held: { folder: string; tokenizer: string; paths: string[]; bytes: number } | null = null;
  let timer: ReturnType<typeof setTimeout> | null = null;
  let cancelled = false;
  const results = new Map<string, Stored>();
  const seen = new Map<string, Map<string, string>>();
  const probe: FoldersProbe = {
    thresholds: { ...STANDARD },
    stepMs: 15,
    steps: 4,
    calls: [],
    touch: (path) => {
      let told = false;
      for (const [folder, stored] of results) {
        if (!stored.stale && covers(folder, path)) {
          stored.stale = true;
          told = true;
        }
      }
      if (told) emit("token-count:folders", badges());
    },
    results: () => [...results.keys()],
    reset: () => {
      stop();
      cancelled = true;
      held = null;
      results.clear();
      seen.clear();
      job = { state: "idle" };
    },
  };

  function set(next: FolderJob): FolderJob {
    job = next;
    emit("token-count:folder", { window: "main", job });
    return job;
  }

  function badges(): FolderBadges {
    return {
      root: root(),
      folders: [...results.entries()]
        .sort(([a], [b]) => (a < b ? -1 : 1))
        .map(([folder, s]) => ({
          folder,
          tokens: s.summary.tokens,
          approx: s.summary.approx,
          stale: s.stale,
          tokenizer: s.summary.tokenizer,
        })),
    };
  }

  function stop(): void {
    if (timer !== null) clearTimeout(timer);
    timer = null;
  }

  function skipOf(path: string, text: string): FolderSkip | null {
    if (BINARY.has(extOf(path))) return "binary";
    if (text.includes("\0")) return "binary";
    if (text.length > 5_000_000) return "tooLarge";
    return null;
  }

  function scan(folder: string, auto: boolean, tokenizer: string): FolderJob {
    const t = probe.thresholds;
    const below = [...files().keys()].filter((p) => covers(folder, p)).sort();
    let bytes = 0;
    const readable: string[] = [];
    for (const path of below) {
      if (skipOf(path, files().get(path) ?? "") === null) {
        readable.push(path);
        bytes += (files().get(path) ?? "").length;
      }
    }
    const limits = auto
      ? { files: t.autoFiles, bytes: t.autoBytes }
      : { files: t.refuseFiles, bytes: t.refuseBytes };
    if (below.length > limits.files || bytes > limits.bytes) {
      if (auto) return set({ state: "idle" });
      return set({
        state: "refused",
        folder,
        files: Math.min(below.length, limits.files + 1),
        bytes,
        reason: below.length > limits.files ? "files" : "bytes",
      });
    }
    const big = !auto && (below.length > t.confirmFiles || bytes > t.confirmBytes);
    held = { folder, tokenizer, paths: below, bytes };
    if (big) {
      return set({
        state: "confirm",
        folder,
        files: below.length,
        bytes,
        seconds: Math.max(1, Math.ceil(below.length / 1000 + bytes / 20_000_000)),
      });
    }
    return begin();
  }

  function begin(): FolderJob {
    const run = held;
    held = null;
    if (run === null) return job;
    const approx = !exact(run.tokenizer) || !ready(run.tokenizer);
    const total = run.paths.length;
    cancelled = false;
    const chunk = Math.max(1, Math.ceil(total / probe.steps));
    let done = 0;
    const step = (): void => {
      if (cancelled) return;
      done = Math.min(total, done + chunk);
      if (done < total) {
        set({
          state: "counting",
          folder: run.folder,
          done,
          total,
          tokenizer: run.tokenizer,
          approx,
        });
        timer = setTimeout(step, probe.stepMs);
        return;
      }
      finish(run, approx);
    };
    set({
      state: "counting",
      folder: run.folder,
      done: 0,
      total,
      tokenizer: run.tokenizer,
      approx,
    });
    timer = setTimeout(step, probe.stepMs);
    return job;
  }

  function finish(
    run: { folder: string; tokenizer: string; paths: string[] },
    approx: boolean,
  ): void {
    const basis = `${run.tokenizer}${approx && exact(run.tokenizer) ? "~" : ""}`;
    const known = seen.get(basis) ?? new Map<string, string>();
    seen.set(basis, known);
    let tokens = 0;
    let counted = 0;
    let cached = 0;
    const skipped: { path: string; reason: FolderSkip }[] = [];
    const types = new Map<string, { files: number; tokens: number }>();
    const per: { path: string; tokens: number }[] = [];
    for (const path of run.paths) {
      const text = files().get(path) ?? "";
      const reason = skipOf(path, text);
      if (reason !== null) {
        skipped.push({ path, reason });
        continue;
      }
      const n = count(run.tokenizer, text);
      if (known.get(path) === text) cached += 1;
      known.set(path, text);
      tokens += n;
      counted += 1;
      per.push({ path, tokens: n });
      const row = types.get(extOf(path)) ?? { files: 0, tokens: 0 };
      row.files += 1;
      row.tokens += n;
      types.set(extOf(path), row);
    }
    const count_of = (reason: FolderSkip): number =>
      skipped.filter((s) => s.reason === reason).length;
    const summary: FolderSummary = {
      folder: run.folder,
      tokenizer: run.tokenizer,
      approx,
      tokens,
      counted,
      skipped: skipped.length,
      skippedBinary: count_of("binary"),
      skippedLarge: count_of("tooLarge"),
      skippedUnreadable: count_of("unreadable"),
      skippedList: skipped.slice(0, 100),
      byType: [...types.entries()]
        .map(([ext, row]) => ({ ext, ...row }))
        .sort((a, b) => b.tokens - a.tokens || (a.ext < b.ext ? -1 : 1))
        .slice(0, 8),
      top: per.sort((a, b) => b.tokens - a.tokens || (a.path < b.path ? -1 : 1)).slice(0, 10),
      millis: probe.stepMs * probe.steps,
      cached,
    };
    results.set(run.folder, { summary, stale: false });
    emit("token-count:folders", badges());
    set({ state: "done", folder: run.folder });
  }

  const handlers: {
    [M in keyof FolderMethods]: (args: FolderMethods[M]["args"]) => FolderMethods[M]["result"];
  } = {
    "token-count.folderStart": ({ folder, tokenizer, auto }) => {
      probe.calls.push(`folderStart ${folder} ${tokenizer}${auto === true ? " auto" : ""}`);
      if (["scanning", "confirm", "counting"].includes(job.state)) return job;
      set({ state: "scanning", folder });
      // the real scan runs on a thread: the answer is `scanning`, the next state arrives as an event
      const first = job;
      timer = setTimeout(() => scan(folder, auto === true, tokenizer), probe.stepMs);
      return first;
    },
    "token-count.folderConfirm": () => {
      probe.calls.push("folderConfirm");
      if (job.state !== "confirm") return job;
      return begin();
    },
    "token-count.folderCancel": () => {
      probe.calls.push("folderCancel");
      if (job.state === "confirm") {
        held = null;
        return set({ state: "idle" });
      }
      if (job.state === "scanning" || job.state === "counting") {
        cancelled = true;
        stop();
        const folder = job.folder;
        // the real thread ends at its next file
        timer = setTimeout(() => set({ state: "cancelled", folder }), probe.stepMs);
      }
      return job;
    },
    "token-count.folderDismiss": () => {
      probe.calls.push("folderDismiss");
      if (["scanning", "confirm", "counting"].includes(job.state)) return job;
      return set({ state: "idle" });
    },
    "token-count.folderJob": () => job,
    "token-count.folders": () => badges(),
    "token-count.folderSummary": ({ folder }) => results.get(folder)?.summary ?? null,
  };

  return { handlers, probe, stop };
}

// The mock backend's half of "drop into project": plans, applies (with progress and cancel) and undo on the in-memory
// project. It mirrors the shapes and the main rules of the real backend (conflicts without case, `name (2)`, refusals),
// not every detail: the Rust side has those, with their tests.

import { baseName } from "../core/explorer/importModel";
import { nameOf } from "../core/explorer/list";
import type { MockProject } from "./mockFs";
import type {
  Api,
  ConflictChoice,
  EventName,
  Events,
  ImportFailure,
  ImportItem,
  ImportMode,
  ImportOutcome,
  ImportPlan,
  ImportRefused,
  Result,
} from "./types";

/** What lies outside the project, for the mock: a dropped path names a file unless the test says it is a folder. */
export type MockSource = { kind: "file" | "folder"; files?: number; link?: boolean };

export type ImportProbe = {
  /** Sources the test describes by absolute path (the rest are one-file entries). */
  outside: Map<string, MockSource>;
  /** Plans asked for: target + sources, in order. */
  planned: { target: string; sources: string[] }[];
  /** Applies done: plan, mode, choice. */
  applied: { planId: number; mode: ImportMode; conflicts: ConflictChoice }[];
  /** While set, an apply waits for it (a test holds an import running, then cancels or lets it end). */
  hold: Promise<void> | null;
  /** Names of items that fail when applied (the error kind the real backend would answer is `denied`). */
  failing: Set<string>;
  /** Cancels asked for. */
  cancelled: number[];
};

const ok = <T>(value: T): Promise<Result<T>> => Promise.resolve({ ok: true, value });

/** The same rule as the backend for "taken": no case, one Unicode form. */
function key(name: string): string {
  return name.normalize("NFC").toLowerCase();
}

function numbered(name: string, folder: boolean, n: number): string {
  const dot = name.lastIndexOf(".");
  return !folder && dot > 0 ? `${name.slice(0, dot)} (${n})${name.slice(dot)}` : `${name} (${n})`;
}

export function createMockImport(
  project: MockProject,
  emit: <E extends EventName>(event: E, payload: Events[E]) => void,
  refresh: () => void,
): {
  api: Pick<Api, "planImport" | "applyImport" | "cancelImport">;
  probe: ImportProbe;
} {
  const probe: ImportProbe = {
    outside: new Map(),
    planned: [],
    applied: [],
    hold: null,
    failing: new Set(),
    cancelled: [],
  };
  const plans = new Map<number, ImportPlan>();
  const cancel = new Set<number>();
  let next = 1;

  const join = (folder: string, name: string): string =>
    folder === "" ? name : `${folder}/${name}`;

  return {
    probe,
    api: {
      planImport: (target, sources) => {
        probe.planned.push({ target, sources });
        const taken = new Set<string>();
        for (const path of [...project.files.keys(), ...project.dirs]) {
          const inside = path.startsWith(target === "" ? "" : `${target}/`);
          if (inside && !path.slice(target === "" ? 0 : target.length + 1).includes("/")) {
            taken.add(key(nameOf(path)));
          }
        }
        const items: ImportItem[] = [];
        const refused: ImportRefused[] = [];
        for (const source of sources) {
          const name = baseName(source);
          const described = probe.outside.get(source);
          if (name === ".git" || name === ".permark") {
            refused.push({ source, name, reason: "protected" });
            continue;
          }
          if (described?.link) {
            refused.push({ source, name, reason: "link" });
            continue;
          }
          const conflict = taken.has(key(name));
          taken.add(key(name));
          items.push({
            source,
            name,
            kind: (described?.kind ?? "file") === "folder" ? "folder" : "file",
            inProject: source.startsWith("C:/Users/demo/permark-docs/"),
            conflict,
          });
        }
        const plan: ImportPlan = { planId: next++, target, items, refused };
        plans.set(plan.planId, plan);
        return ok(plan);
      },

      applyImport: async (planId, mode, conflicts) => {
        probe.applied.push({ planId, mode, conflicts });
        const plan = plans.get(planId);
        if (!plan) {
          return {
            ok: false,
            error: {
              kind: "backend",
              command: "fileops_import_apply",
              error: { kind: "unavailable", subject: null, detail: null },
            },
          };
        }
        plans.delete(planId);
        if (mode === "move" && plan.items.some((i) => i.inProject)) {
          return {
            ok: false,
            error: {
              kind: "backend",
              command: "fileops_import_apply",
              error: { kind: "unavailable", subject: null, detail: null },
            },
          };
        }
        const created: string[] = [];
        const failed: ImportFailure[] = [];
        let skipped = 0;
        let files = 0;
        let cancelled = false;
        const total = plan.items.length;
        const emitProgress = (done: number, item: string): void =>
          emit("import-progress", {
            planId,
            phase: "copying",
            itemsDone: done,
            itemsTotal: total,
            filesDone: files,
            filesTotal: total,
            bytesDone: files * 1000,
            bytesTotal: total * 1000,
            item,
          });
        emitProgress(0, plan.items[0]?.name ?? "");
        if (probe.hold) await probe.hold;
        for (const [at, item] of plan.items.entries()) {
          if (cancel.has(planId)) {
            cancelled = true;
            break;
          }
          let name = item.name;
          const taken = (n: string): boolean =>
            project.exists(join(plan.target, n)) ||
            [...project.files.keys(), ...project.dirs].some(
              (p) => key(p) === key(join(plan.target, n)),
            );
          if (taken(name)) {
            if (conflicts === "skip") {
              skipped += 1;
              continue;
            }
            let n = 2;
            while (taken(numbered(item.name, item.kind === "folder", n))) n += 1;
            name = numbered(item.name, item.kind === "folder", n);
          }
          if (probe.failing.has(item.name)) {
            failed.push({
              name: item.name,
              error: { kind: "denied", subject: item.source, detail: null },
            });
            continue;
          }
          const path = join(plan.target, name);
          if (item.kind === "folder") {
            project.dirs.add(path);
            project.kept.add(path);
            const count = probe.outside.get(item.source)?.files ?? 1;
            for (let n = 1; n <= count; n += 1) project.add(`${path}/file-${n}.txt`, "x");
            files += count;
          } else {
            project.add(path, "");
            files += 1;
          }
          created.push(path);
          emitProgress(at + 1, item.name);
        }
        if (created.length > 0) project.recordImport(created);
        refresh();
        cancel.delete(planId);
        const outcome: ImportOutcome = {
          target: plan.target,
          copied: mode === "copy" ? created.length : 0,
          moved: mode === "move" ? created.length : 0,
          files,
          bytes: files * 1000,
          skipped,
          failed,
          links: 0,
          leftBehind: 0,
          cancelled,
        };
        return { ok: true, value: outcome };
      },

      cancelImport: (planId) => {
        probe.cancelled.push(planId);
        cancel.add(planId);
        return ok(true);
      },
    },
  };
}

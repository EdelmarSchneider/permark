// Drop from outside onto the Explorer, the view's side: the question at the drop point (Copy here | Move here, names
// that exist, what cannot come in), the running import and its end. The backend owns the work and the undo; this asks,
// shows progress and words the result. One import per view. States are tagged unions.

import type {
  ApiError,
  Api,
  ConflictChoice,
  ImportMode,
  ImportPlan,
  ImportProgress,
  Unlisten,
} from "../../api";
import { t } from "../i18n";
import { addedAnything, canMove, folderLabel, outcomeText } from "./importModel";

/** A point of the view, CSS px. */
export type Point = { x: number; y: number };

export type ImportState =
  | { status: "idle" }
  /** The plan is on its way; after a moment (`slow`) the question shows already, saying it is checking names. */
  | { status: "planning"; folder: string; where: string; at: Point; slow: boolean }
  | {
      status: "asking";
      plan: ImportPlan;
      /** The target folder as the person reads it. */
      where: string;
      at: Point;
      conflicts: ConflictChoice;
    }
  | {
      status: "running";
      plan: ImportPlan;
      where: string;
      mode: ImportMode;
      progress: ImportProgress | null;
      cancelling: boolean;
    };

/** A plan slower than this shows the question before it arrives. */
export const SLOW_PLAN_MS = 300;

export type ImportHost = {
  fail(error: ApiError): void;
  note(text: string): void;
  action(text: string, ...actions: { label: string; run: () => void }[]): void;
  /** The name of the open project (what the project folder is called in sentences). */
  project(): string;
  /** The question is about to show: the view's window comes forward (a drop does not activate it). */
  focus(): void;
  /** The drop is over for the tree: folders the drag opened close, except `kept` (where an import landed). */
  end(kept: string | null): void;
  /** Items put in place (the tree selects them). */
  placed(paths: readonly string[]): void;
  /** Takes the newest file operation back. */
  undo(): Promise<void>;
};

export class ImportFlow {
  readonly #api: Api;
  readonly #host: ImportHost;
  state = $state.raw<ImportState>({ status: "idle" });
  #slow: ReturnType<typeof setTimeout> | null = null;
  /** Tells a plan's answer whether it still matters (a newer drop or a cancel came meanwhile). */
  #asked = 0;

  constructor(api: Api, host: ImportHost) {
    this.#api = api;
    this.#host = host;
  }

  /** Listens to the progress of the running import. Returns the teardown. */
  start(): Promise<Unlisten> {
    return this.#api.on("import-progress", (progress) => {
      const state = this.state;
      if (state.status === "running" && state.plan.planId === progress.planId) {
        this.state = { ...state, progress };
      }
    });
  }

  /** An import is running (a drop onto the Explorer waits for it to finish). */
  get busy(): boolean {
    return this.state.status === "running";
  }

  /** The project is gone or another one is shown: nothing of this drop matters any more. */
  reset(): void {
    this.#stopSlow();
    this.#asked += 1;
    this.state = { status: "idle" };
  }

  /** Files | folders dropped on the Explorer at `at`, into `folder`: the question, once the plan is back. */
  async plan(folder: string, paths: readonly string[], at: Point): Promise<void> {
    if (this.state.status === "running") {
      this.#host.note(t("import.busy"));
      this.#host.end(null);
      return;
    }
    this.discard();
    const asked = ++this.#asked;
    const where = folderLabel(folder, this.#host.project());
    this.state = { status: "planning", folder, where, at, slow: false };
    this.#slow = setTimeout(() => {
      this.#slow = null;
      const state = this.state;
      if (state.status !== "planning" || this.#asked !== asked) return;
      this.#host.focus();
      this.state = { ...state, slow: true };
    }, SLOW_PLAN_MS);
    const planned = await this.#api.planImport(folder, [...paths]);
    if (this.#asked !== asked) return;
    const slow = this.state.status === "planning" && this.state.slow;
    this.#stopSlow();
    if (!planned.ok) {
      this.state = { status: "idle" };
      this.#host.end(null);
      this.#host.fail(planned.error);
      return;
    }
    if (!slow) this.#host.focus();
    // Keep both comes preselected; Enter on the question copies (never takes anything away from where it is)
    this.state = { status: "asking", plan: planned.value, where, at, conflicts: "keepBoth" };
  }

  setConflicts(conflicts: ConflictChoice): void {
    const state = this.state;
    if (state.status === "asking") this.state = { ...state, conflicts };
  }

  /** The person closed the question (Cancel | Esc | outside click | the view hidden): nothing happens. */
  cancel(): void {
    if (this.state.status !== "planning" && this.state.status !== "asking") return;
    this.discard();
    this.#host.end(null);
  }

  /** A newer drop replaces the question (the tree's folders stay as the new drag left them). */
  discard(): void {
    if (this.state.status !== "planning" && this.state.status !== "asking") return;
    this.#stopSlow();
    this.#asked += 1;
    this.state = { status: "idle" };
  }

  /** Copy here | Move here. */
  async confirm(mode: ImportMode): Promise<void> {
    const asked = this.state;
    if (asked.status !== "asking" || asked.plan.items.length === 0) return;
    if (mode === "move" && !canMove(asked.plan)) return;
    this.state = {
      status: "running",
      plan: asked.plan,
      where: asked.where,
      mode,
      progress: null,
      cancelling: false,
    };
    const result = await this.#api.applyImport(asked.plan.planId, mode, asked.conflicts);
    if (this.state.status === "running") this.state = { status: "idle" };
    if (!result.ok) {
      this.#host.end(null);
      this.#host.fail(result.error);
      return;
    }
    const outcome = result.value;
    const text = outcomeText(outcome, asked.where);
    if (addedAnything(outcome)) {
      this.#host.end(asked.plan.target);
      this.#host.placed(outcome.placed);
      this.#host.action(text, { label: t("fileOps.undo"), run: () => void this.#host.undo() });
    } else {
      this.#host.end(null);
      this.#host.note(text);
    }
  }

  /** Stops the running import; what was finished stays. */
  async stop(): Promise<void> {
    const state = this.state;
    if (state.status !== "running" || state.cancelling) return;
    this.state = { ...state, cancelling: true };
    await this.#api.cancelImport(state.plan.planId);
  }

  #stopSlow(): void {
    if (this.#slow !== null) clearTimeout(this.#slow);
    this.#slow = null;
  }
}

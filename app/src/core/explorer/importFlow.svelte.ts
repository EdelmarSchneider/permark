// Drop into project, the window's side: what a drag over the window says, where a drop goes (pictures into the editor,
// everything else into a folder), the question (Copy | Move, taken names), the running import and its end. The backend
// owns the work and the undo; this asks, shows progress and words the result. States are tagged unions.

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
import type { Spot } from "./dropTarget";
import {
  addedAnything,
  baseName,
  canMove,
  folderLabel,
  nothingText,
  outcomeText,
  pictureType,
  routeDrop,
} from "./importModel";

export type ImportState =
  | { status: "idle" }
  | { status: "planning" }
  | {
      status: "asking";
      plan: ImportPlan;
      /** The target folder as the person reads it. */
      where: string;
      mode: ImportMode;
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

/** What a drag over the window would do: where it lands, or that it puts pictures into the editor. */
export type Hover = { folder: string; pictures: boolean; count: number };

export type ImportHost = {
  fail(error: ApiError): void;
  note(text: string): void;
  action(text: string, ...actions: { label: string; run: () => void }[]): void;
  /** The name of the open project (what the project folder is called in sentences). */
  project(): string;
  /** What is under a point of the window. */
  spot(x: number, y: number): Spot;
  /** The tree marks (or, `null`, stops marking) the folder an outside drag is over. */
  mark(folder: string | null): void;
  /** The editor on screen takes pictures dropped at a point; whether one did. */
  pictures(files: File[], x: number, y: number): boolean;
  /** Takes the newest file operation back. */
  undo(): Promise<void>;
};

export class ImportFlow {
  readonly #api: Api;
  readonly #host: ImportHost;
  state = $state.raw<ImportState>({ status: "idle" });
  hover = $state.raw<Hover | null>(null);

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

  /** The project is gone or another one is shown: nothing of this drop matters any more. */
  reset(): void {
    this.state = { status: "idle" };
    this.#clearHover();
  }

  // ---- while the drag is over the window --------------------------------------------------------------------------

  over(x: number, y: number, paths: readonly string[], count: number): void {
    const spot = this.#host.spot(x, y);
    const pictures = routeDrop(paths, spot.editor).kind === "pictures";
    const before = this.hover;
    if (
      before === null ||
      before.folder !== spot.folder ||
      before.pictures !== pictures ||
      before.count !== count
    ) {
      this.hover = { folder: spot.folder, pictures, count };
    }
    if (before === null || before.folder !== spot.folder || before.pictures !== pictures) {
      this.#host.mark(pictures ? null : spot.folder);
    }
  }

  leave(): void {
    this.#clearHover();
  }

  #clearHover(): void {
    if (this.hover !== null) this.hover = null;
    this.#host.mark(null);
  }

  // ---- the drop ---------------------------------------------------------------------------------------------------

  async drop(paths: readonly string[], x: number, y: number): Promise<void> {
    this.#clearHover();
    if (paths.length === 0) return;
    const spot = this.#host.spot(x, y);
    if (routeDrop(paths, spot.editor).kind === "pictures") {
      await this.#pictures(paths, x, y);
      return;
    }
    await this.#plan(spot.folder, paths);
  }

  async #pictures(paths: readonly string[], x: number, y: number): Promise<void> {
    const files: File[] = [];
    for (const path of paths) {
      const read = await this.#api.readPicture(path);
      if (!read.ok) {
        this.#host.fail(read.error);
        continue;
      }
      files.push(new File([read.value], baseName(path), { type: pictureType(path) }));
    }
    if (files.length > 0) this.#host.pictures(files, x, y);
  }

  async #plan(folder: string, paths: readonly string[]): Promise<void> {
    if (this.state.status !== "idle") {
      this.#host.note(t("import.busy"));
      return;
    }
    this.state = { status: "planning" };
    const planned = await this.#api.planImport(folder, [...paths]);
    if (!planned.ok) {
      this.state = { status: "idle" };
      this.#host.fail(planned.error);
      return;
    }
    const plan = planned.value;
    if (plan.items.length === 0) {
      this.state = { status: "idle" };
      this.#host.note(nothingText(plan));
      return;
    }
    this.state = {
      status: "asking",
      plan,
      where: folderLabel(plan.target, this.#host.project()),
      // Copy comes preselected, always: Enter on the dialog never takes anything away from where it is
      mode: "copy",
      conflicts: "keepBoth",
    };
  }

  // ---- the question -----------------------------------------------------------------------------------------------

  setMode(mode: ImportMode): void {
    const state = this.state;
    if (state.status === "asking" && (mode === "copy" || canMove(state.plan))) {
      this.state = { ...state, mode };
    }
  }

  setConflicts(conflicts: ConflictChoice): void {
    const state = this.state;
    if (state.status === "asking") this.state = { ...state, conflicts };
  }

  cancelAsking(): void {
    if (this.state.status === "asking") this.state = { status: "idle" };
  }

  async confirm(): Promise<void> {
    const asked = this.state;
    if (asked.status !== "asking") return;
    this.state = {
      status: "running",
      plan: asked.plan,
      where: asked.where,
      mode: asked.mode,
      progress: null,
      cancelling: false,
    };
    const result = await this.#api.applyImport(asked.plan.planId, asked.mode, asked.conflicts);
    this.state = { status: "idle" };
    if (!result.ok) {
      this.#host.fail(result.error);
      return;
    }
    const text = outcomeText(result.value, asked.where);
    if (addedAnything(result.value)) {
      this.#host.action(text, { label: t("fileOps.undo"), run: () => void this.#host.undo() });
    } else {
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
}

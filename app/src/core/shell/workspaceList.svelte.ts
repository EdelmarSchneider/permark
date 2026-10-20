// The workspaces the process has open (every window's), as the switcher bar and its keys read them. The backend owns
// the list; this keeps the last one it sent.

import { api, type Api, type Unlisten, type WorkspaceSummary } from "../../api";
import { afterClose, canSwitch, neighbor } from "./switcherModel";

class Switcher {
  list = $state.raw<readonly WorkspaceSummary[]>([]);
  #api: Api = api;

  /** There is another workspace to go to (next | previous). */
  get canSwitch(): boolean {
    return canSwitch(this.list);
  }

  /** Listens, then reads the list (nothing is lost in between). Returns the teardown. */
  start(backend: Api = api): Unlisten {
    this.#api = backend;
    let gone = false;
    let off: Unlisten | null = null;
    void backend
      .on("workspaces-changed", (event) => {
        this.list = event.workspaces;
      })
      .then(async (stop) => {
        if (gone) {
          stop();
          return;
        }
        off = stop;
        const listed = await backend.listWorkspaces();
        if (!gone && listed.ok) this.list = listed.value;
      });
    return () => {
      gone = true;
      off?.();
    };
  }

  focus(id: number): void {
    void this.#api.focusWorkspace(id);
  }

  /** The previous | next workspace in list order (wraps) after `current`, the one this window shows (`null`: none). */
  focusNeighbor(step: 1 | -1, current: number | null): void {
    const next = neighbor(this.list, current, step);
    if (next !== null && next.id !== current) this.focus(next.id);
  }

  /** A new window on the same workspace. */
  openInNewWindow(workspace: WorkspaceSummary): void {
    if (workspace.kind === "notes") void this.#api.openNotes("newWindow");
    else void this.#api.openPath(workspace.path, "newWindow");
  }

  /** Asks every window that shows it to close it (each asks about unsaved edits first). */
  close(id: number): void {
    void this.#api.workspaceClose(id);
  }

  /** The workspace to show after `closed` goes away (`null`: none left). */
  afterClose(closed: number): WorkspaceSummary | null {
    return afterClose(this.list, closed);
  }

  /** The open workspace of a folder | file path, if any (paths compared case-folded with `/`). */
  findPath(path: string): WorkspaceSummary | null {
    const key = (p: string): string => p.replaceAll("\\", "/").replace(/\/+$/, "").toLowerCase();
    return this.list.find((w) => w.kind !== "notes" && key(w.path) === key(path)) ?? null;
  }

  get notes(): WorkspaceSummary | null {
    return this.list.find((w) => w.kind === "notes") ?? null;
  }
}

export const switcher = new Switcher();
export type { Switcher };

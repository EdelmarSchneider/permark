// Drop from outside, the view's side (`explorer.md` Drop from outside): what a drag over the view would do (the badge
// near the pointer, the Explorer's mark), and where a drop goes: pictures into the editor, into the project (the
// import question), loose files of the Notes, or tabs. Nothing covers the view while dragging. States are tagged
// unions; every timer | frame is torn down on leave | drop | the view hidden | teardown.

import type { Api, ApiError, DropEvent } from "../../api";
import { t } from "../i18n";
import {
  badgeOf,
  baseName,
  OPEN_ASK_ABOVE,
  openFailedText,
  pictureType,
  routeDrop,
  type Badge,
  type DropContext,
  type DropRoute,
  type DropSpot,
} from "../explorer/importModel";

export type Point = { x: number; y: number };

/** What the badge shows while a drag is over the view. */
export type Hover = { route: DropRoute; badge: Badge; at: Point; count: number };

/** Many paths dropped to open: asked first. */
export type OpenAsk = { status: "none" } | { status: "asking"; paths: string[]; at: Point };

/** The Notes tab takes some files as loose files (set by the Notes while they show). */
export type NotesDrop = {
  takes(path: string): boolean;
  open(paths: string[]): Promise<void>;
};

/** The import question of the view (`importFlow`). */
export type Imports = {
  readonly busy: boolean;
  /** Its question is open or on its way (the tree keeps the folders it is about). */
  readonly waiting: boolean;
  plan(folder: string, paths: readonly string[], at: Point): Promise<void>;
  cancel(): void;
  discard(): void;
};

/** The tree's side of an outside drag (`treeDrop`). */
export type TreeDrop = {
  over(folder: string | null, point: Point | null): void;
  hold(): void;
  end(kept: string | null): void;
};

export type DropHost = {
  /** What the view shows. */
  workspace(): DropContext["workspace"];
  /** What is under a point of the view. */
  spot(x: number, y: number): DropSpot;
  /** An import target as the person reads it. */
  folderName(folder: string): string;
  tree: TreeDrop;
  imports: Imports;
  /** The editor on screen takes pictures dropped at a point; whether it did. */
  pictures(files: File[], at: Point): boolean;
  fail(error: ApiError): void;
  note(text: string): void;
};

/** Runs `run` on the next frame; returns its cancel. */
export type Frames = (run: () => void) => () => void;

const browserFrames: Frames = (run) => {
  const id = requestAnimationFrame(run);
  return () => cancelAnimationFrame(id);
};

export class DropFlow {
  readonly #api: Api;
  readonly #host: DropHost;
  readonly #frames: Frames;
  hover = $state.raw<Hover | null>(null);
  ask = $state.raw<OpenAsk>({ status: "none" });
  /** The Notes, while they show in this view. */
  notes: NotesDrop | null = null;
  /** The newest `over` not drawn yet (one update per frame, however many events). */
  #pending: { at: Point; paths: readonly string[] } | null = null;
  #cancelFrame: (() => void) | null = null;

  constructor(api: Api, host: DropHost, frames: Frames = browserFrames) {
    this.#api = api;
    this.#host = host;
    this.#frames = frames;
  }

  /** One event of the system's drag. */
  event(event: DropEvent): void {
    switch (event.type) {
      case "over":
        this.over(event.x, event.y, event.paths);
        return;
      case "leave":
        this.leave();
        return;
      case "drop":
        void this.drop(event.paths, event.x, event.y);
        return;
      default:
        event satisfies never;
    }
  }

  #context(): DropContext {
    const notes = this.notes;
    return {
      workspace: this.#host.workspace(),
      notesTake: notes === null ? null : (path) => notes.takes(path),
    };
  }

  #route(at: Point, paths: readonly string[]): DropRoute {
    return routeDrop(this.#host.spot(at.x, at.y), paths, this.#context());
  }

  // ---- while the drag is over the view -------------------------------------------------------------------------------

  over(x: number, y: number, paths: readonly string[]): void {
    this.#pending = { at: { x, y }, paths };
    this.#cancelFrame ??= this.#frames(() => {
      this.#cancelFrame = null;
      const pending = this.#pending;
      this.#pending = null;
      if (pending !== null) this.#draw(pending.at, pending.paths);
    });
  }

  #draw(at: Point, paths: readonly string[]): void {
    const route = this.#route(at, paths);
    const busy = this.#host.imports.busy;
    const folder = route.kind === "import" ? this.#host.folderName(route.folder) : "";
    const badge = badgeOf(route, paths.length, folder, busy);
    this.hover = { route, badge, at, count: paths.length };
    if (route.kind === "import") this.#host.tree.over(busy ? null : route.folder, at);
    else this.#host.tree.over(null, null);
  }

  /** The drag left the view without a drop. While an import question waits, only the mark goes: the folders it is
   *  about stay open (they close when the question does). */
  leave(): void {
    this.#stopFrame();
    this.hover = null;
    if (this.#host.imports.waiting) this.#host.tree.over(null, null);
    else this.#host.tree.end(null);
  }

  #stopFrame(): void {
    this.#cancelFrame?.();
    this.#cancelFrame = null;
    this.#pending = null;
  }

  // ---- the drop -------------------------------------------------------------------------------------------------------

  async drop(paths: readonly string[], x: number, y: number): Promise<void> {
    this.#stopFrame();
    this.hover = null;
    // a new drop closes an open question, whatever it lands on
    this.ask = { status: "none" };
    this.#host.imports.discard();
    const at = { x, y };
    if (paths.length === 0) {
      this.#host.tree.end(null);
      return;
    }
    const route = this.#route(at, paths);
    switch (route.kind) {
      case "pictures":
        this.#host.tree.end(null);
        await this.#pictures(paths, at);
        return;
      case "import":
        if (this.#host.imports.busy) {
          this.#host.tree.end(null);
          this.#host.note(t("import.busy"));
          return;
        }
        this.#host.tree.hold();
        await this.#host.imports.plan(route.folder, paths, at);
        return;
      case "notes":
        this.#host.tree.end(null);
        await this.#notes([...paths], at);
        return;
      case "open":
        this.#host.tree.end(null);
        await this.#open([...paths], at);
        return;
      default:
        route satisfies never;
    }
  }

  async #pictures(paths: readonly string[], at: Point): Promise<void> {
    const files: File[] = [];
    for (const path of paths) {
      const read = await this.#api.readPicture(path);
      if (!read.ok) {
        this.#host.fail(read.error);
        continue;
      }
      files.push(new File([read.value], baseName(path), { type: pictureType(path) }));
    }
    if (files.length > 0) this.#host.pictures(files, at);
  }

  /** The Notes tab: paths open somewhere come forward (one place); files PerMark opens as text become loose files
   *  (the backend looks at each file: never a folder); folders and other files open as tabs. */
  async #notes(paths: string[], at: Point): Promise<void> {
    const notes = this.notes;
    if (notes === null) {
      await this.#open(paths, at);
      return;
    }
    const placed = await this.#api.openDropped(paths, "existingOnly");
    if (!placed.ok) {
      this.#host.fail(placed.error);
      return;
    }
    this.#failed(placed.value.failed);
    const left = placed.value.left;
    const text = left.filter((l) => l.text).map((l) => l.path);
    if (text.length > 0) await notes.open(text);
    await this.#open(
      left.filter((l) => !l.text).map((l) => l.path),
      at,
    );
  }

  async #open(paths: string[], at: Point): Promise<void> {
    if (paths.length === 0) return;
    if (paths.length > OPEN_ASK_ABOVE) {
      void this.#api.focusView();
      this.ask = { status: "asking", paths, at };
      return;
    }
    await this.#openNow(paths);
  }

  /** "Open them". */
  async confirmOpen(): Promise<void> {
    const ask = this.ask;
    if (ask.status !== "asking") return;
    this.ask = { status: "none" };
    await this.#openNow(ask.paths);
  }

  cancelOpen(): void {
    this.ask = { status: "none" };
  }

  async #openNow(paths: string[]): Promise<void> {
    const opened = await this.#api.openDropped(paths, "tabs");
    if (!opened.ok) {
      this.#host.fail(opened.error);
      return;
    }
    this.#failed(opened.value.failed);
  }

  #failed(failed: Parameters<typeof openFailedText>[0]): void {
    const text = openFailedText(failed);
    if (text !== null) this.#host.note(text);
  }

  // ---- the view goes ---------------------------------------------------------------------------------------------------

  /** The view is no longer on screen (a tab switch): the drag and every question end. */
  hidden(): void {
    this.leave();
    this.ask = { status: "none" };
    this.#host.imports.cancel();
  }

  /** Teardown: nothing keeps running. */
  reset(): void {
    this.#stopFrame();
    this.hover = null;
    this.ask = { status: "none" };
  }
}

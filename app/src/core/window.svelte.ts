// This window's state + the open flow: boot, open folder | file | recent | drop, "This window | New window",
// close back to Welcome, settings changes, notices. Backend owns the truth (`window-changed`); this mirrors it.

import {
  api,
  type Api,
  type ApiError,
  type DropEvent,
  type GitBackendPref,
  type IpcError,
  type LinkUpdate,
  type OpenIn,
  type OpenPlacement,
  type RecentFolder,
  type StartIn,
  type SettingChange,
  type StateRecovered,
  type Unlisten,
  type WindowView,
} from "../api";
import { conditions } from "./conditions/context.svelte";
import { restartPlugins, syncPlugins } from "./plugins/instance.svelte";
import { language, parsePreference, t } from "./i18n";
import { look } from "./look.svelte";
import { ui } from "./shell/ui.svelte";

export type NoticeAction = { label: string; run: () => void };

export type Notice =
  | { id: number; kind: "openFailed"; path: string; error: IpcError }
  | { id: number; kind: "apiFailed"; path: string | null; error: ApiError }
  | { id: number; kind: "dropOne" }
  | { id: number; kind: "stateRecovered"; notice: StateRecovered }
  | { id: number; kind: "settingNotSaved"; error: ApiError }
  | { id: number; kind: "initFailed"; path: string; error: ApiError }
  | { id: number; kind: "fileOpFailed"; error: ApiError }
  | { id: number; kind: "note"; text: string }
  /** A quiet line with buttons (Undo, Open in project). */
  | { id: number; kind: "action"; text: string; actions: readonly NoticeAction[] };

type NoticeBody = Notice extends infer N ? (N extends Notice ? Omit<N, "id"> : never) : never;

/** `path: null` = the Notes. */
export type Choice = { status: "closed" } | { status: "asking"; path: string | null };
export type Drag = { status: "idle" } | { status: "over"; count: number };

/** A project window takes what is dragged over it and dropped on it (into the project, or into an editor). */
export type ProjectDrop = {
  over(x: number, y: number, paths: readonly string[], count: number): void;
  leave(): void;
  drop(paths: readonly string[], x: number, y: number): void;
};
export type Boot =
  { status: "booting" } | { status: "ready" } | { status: "failed"; error: ApiError };

export class WindowStore {
  readonly #api: Api;
  boot = $state.raw<Boot>({ status: "booting" });
  label = $state("");
  view = $state.raw<WindowView>({ state: "welcome" });
  recent = $state.raw<readonly RecentFolder[]>([]);
  choice = $state.raw<Choice>({ status: "closed" });
  drag = $state.raw<Drag>({ status: "idle" });
  notices = $state.raw<readonly Notice[]>([]);
  /** Whether a move rewrites the links that point at it (the setting). */
  linkUpdate = $state<LinkUpdate>("ask");
  /** How "copy reference" is written (setting). */
  referenceTemplate = $state("{path}#{section}");
  /** The editor Ctrl+E opens for Markdown. */
  editMode = $state<"edit" | "source">("edit");
  /** Which git the git plugin uses. */
  gitBackend = $state<GitBackendPref>("automatic");
  /** The command that opens a file in the person's editor (setting; empty = not set). */
  editor = $state("");
  /** Where the notes are (setting; empty = the default folder). */
  notesFolder = $state("");
  /** What the app starts on. */
  startIn = $state<StartIn>("welcome");
  /** What opening over a shown workspace does: ask, reuse this window, open a new one. */
  openIn = $state<OpenIn>("ask");
  /** The person's keyboard map: command id -> combo (`""` = unbound). */
  keymap = $state.raw<Readonly<Record<string, string>>>({});
  #nextNotice = 1;
  /** A window page that takes dropped files itself (the Notes): returns whether it did. */
  dropHook: ((paths: readonly string[]) => boolean) | null = null;
  /** The project this window shows takes dropped files itself (they come into the project). */
  projectDrop: ProjectDrop | null = null;

  constructor(backend: Api) {
    this.#api = backend;
  }

  /** Subscribes, then asks what this window shows (no event lost in between). Returns teardown of listeners. */
  async start(): Promise<Unlisten> {
    const offs = await Promise.all([
      this.#api.on("window-changed", (e) => {
        if (e.window === this.label) this.#showView(e.view);
      }),
      this.#api.on("context-changed", (e) => {
        if (e.window === this.label) this.#applyContext(e.context);
      }),
      this.#api.on("settings-changed", (e) => {
        look.set(e.settings);
        this.linkUpdate = e.settings.linkUpdate;
        this.referenceTemplate = e.settings.referenceTemplate;
        this.editMode = e.settings.editMode;
        this.gitBackend = e.settings.gitBackend;
        this.editor = e.settings.editor;
        this.notesFolder = e.settings.notesFolder;
        this.startIn = e.settings.startIn;
        this.openIn = e.settings.openIn;
        this.keymap = e.settings.keymap;
        void language.choose(parsePreference(e.settings.language));
      }),
      this.#api.on("open-failed", (e) => {
        if (e.window === this.label)
          this.#notify({ kind: "openFailed", path: e.path, error: e.error });
      }),
      this.#api.on("state-recovered", (e) => this.#notify({ kind: "stateRecovered", notice: e })),
      this.#api.onDrop((e) => this.#onDrop(e)),
    ]);
    const booted = await this.#api.windowBoot();
    if (booted.ok) {
      const info = booted.value;
      this.label = info.window;
      this.#applyContext(info.context);
      look.set(info.settings);
      this.linkUpdate = info.settings.linkUpdate;
      this.referenceTemplate = info.settings.referenceTemplate;
      this.editMode = info.settings.editMode;
      this.gitBackend = info.settings.gitBackend;
      this.editor = info.settings.editor;
      this.notesFolder = info.settings.notesFolder;
      this.startIn = info.settings.startIn;
      this.openIn = info.settings.openIn;
      this.keymap = info.settings.keymap;
      await language.choose(parsePreference(info.settings.language));
      for (const notice of info.notices) this.#notify({ kind: "stateRecovered", notice });
      for (const failed of info.failures) {
        this.#notify({ kind: "openFailed", path: failed.path, error: failed.error });
      }
      this.#showView(info.view);
      this.boot = { status: "ready" };
    } else {
      await language.choose("system");
      this.boot = { status: "failed", error: booted.error };
    }
    return () => offs.forEach((off) => off());
  }

  /** The conditions changed: the UI keeps them, and the plugins that came on or went off follow. */
  #applyContext(context: Parameters<typeof conditions.set>[0]): void {
    conditions.set(context);
    syncPlugins(context.plugins);
  }

  #showView(view: WindowView): void {
    const before = this.view;
    this.view = view;
    // focus mode belongs to the workspace it was turned on in
    if (
      view.state === "welcome" ||
      (view.state === "showing" &&
        before.state === "showing" &&
        before.workspace.id !== view.workspace.id)
    ) {
      ui.resetFocus();
    }
    // PerMark was initialized in the project this window shows: its plugins started over on the new state folder
    if (
      before.state === "showing" &&
      view.state === "showing" &&
      before.workspace.kind === "project" &&
      view.workspace.kind === "project" &&
      before.workspace.id === view.workspace.id &&
      before.workspace.stateLocation !== view.workspace.stateLocation
    ) {
      restartPlugins();
    }
    if (view.state === "welcome") void this.refreshRecent();
  }

  async refreshRecent(): Promise<void> {
    const listed = await this.#api.recentList();
    if (listed.ok) this.recent = listed.value;
  }

  async openFolder(): Promise<void> {
    const path = await this.#api.pickFolder(t("dialog.openFolder"));
    if (path) await this.open(path, "ask");
  }

  async openFile(): Promise<void> {
    const path = await this.#api.pickFile(t("dialog.openFile"), t("dialog.filter.docs"));
    if (path) await this.open(path, "ask");
  }

  /** `ask` = what the `open_in` setting says (ask = the dialog when this window shows something). */
  #placement(placement: OpenPlacement): OpenPlacement {
    if (placement !== "ask") return placement;
    switch (this.openIn) {
      case "ask":
        return "ask";
      case "this":
        return "thisWindow";
      case "new":
        return "newWindow";
      default:
        return this.openIn satisfies never;
    }
  }

  async open(path: string, placement: OpenPlacement): Promise<void> {
    await this.#opened(await this.#api.openPath(path, this.#placement(placement)), {
      kind: "path",
      path,
    });
  }

  /** Shows the Notes in this window (asks first when the window shows something). */
  async openNotes(placement: OpenPlacement = "ask"): Promise<void> {
    await this.#opened(await this.#api.openNotes(this.#placement(placement)), {
      kind: "notes",
    });
  }

  async #opened(
    result: Awaited<ReturnType<Api["openPath"]>>,
    what: { kind: "path"; path: string } | { kind: "notes" },
  ): Promise<void> {
    if (!result.ok) {
      // a newer open in this window won: nothing to tell
      if (result.error.kind === "backend" && result.error.error.kind === "superseded") return;
      this.#notify({
        kind: "apiFailed",
        path: what.kind === "path" ? what.path : null,
        error: result.error,
      });
      return;
    }
    const outcome = result.value;
    switch (outcome.outcome) {
      case "opened":
        this.#showView(outcome.view);
        return;
      case "needsChoice":
        this.choice =
          what.kind === "path"
            ? { status: "asking", path: what.path }
            : { status: "asking", path: null };
        return;
      case "openedElsewhere":
        return;
      default:
        outcome satisfies never;
    }
  }

  /** Answer of the "This window | New window" dialog; `null` = cancelled. `remember`: keep the answer as the setting. */
  async answerChoice(answer: "here" | "new" | null, remember = false): Promise<void> {
    const asked = this.choice;
    this.choice = { status: "closed" };
    if (asked.status !== "asking" || answer === null) return;
    if (remember) {
      await this.changeSetting({ key: "openIn", value: answer === "here" ? "this" : "new" });
    }
    const placement = answer === "here" ? "thisWindow" : "newWindow";
    if (asked.path === null) await this.openNotes(placement);
    else await this.open(asked.path, placement);
  }

  async closeWorkspace(): Promise<void> {
    await this.#api.showWelcome();
  }

  /** Creates `.permark/` in a folder; a project open on it reloads at once. `true` when it worked. */
  async initProject(path: string): Promise<boolean> {
    const result = await this.#api.projectInit(path);
    if (!result.ok) {
      this.#notify({ kind: "initFailed", path, error: result.error });
      return false;
    }
    return true;
  }

  /** New project: a folder is picked, PerMark is initialized in it, and it opens like any folder. */
  async newProject(): Promise<void> {
    const path = await this.#api.pickFolder(t("dialog.newProject"));
    if (path === null) return;
    if (await this.initProject(path)) await this.open(path, "ask");
  }

  async removeRecent(path: string): Promise<void> {
    await this.#api.recentRemove(path);
    await this.refreshRecent();
  }

  async changeSetting(change: SettingChange): Promise<void> {
    const result = await this.#api.settingsSet(change);
    if (!result.ok) this.#notify({ kind: "settingNotSaved", error: result.error });
    // success: `settings-changed` reaches every window (this one included)
  }

  /** One setting, with the refusal handed back (the page words it under the control) instead of a notice. */
  async trySetting(change: SettingChange): Promise<ApiError | null> {
    const result = await this.#api.settingsSet(change);
    return result.ok ? null : result.error;
  }

  /** A file operation of the Explorer failed. */
  notifyFileOpFailed(error: ApiError): void {
    this.#notify({ kind: "fileOpFailed", error });
  }

  /** A quiet note ("Moved a.md", "Undone"). */
  notifyNote(text: string): void {
    this.#notify({ kind: "note", text });
  }

  /** A quiet line with buttons ("Deleted a note" + Undo). */
  notifyAction(text: string, ...actions: NoticeAction[]): void {
    this.#notify({ kind: "action", text, actions });
  }

  dismiss(id: number): void {
    this.notices = this.notices.filter((n) => n.id !== id);
  }

  #notify(body: NoticeBody): void {
    const notice = { ...body, id: this.#nextNotice++ };
    // newest last; at most 5 on screen
    this.notices = [...this.notices, notice].slice(-5);
  }

  #onDrop(event: DropEvent): void {
    switch (event.type) {
      case "over":
        this.drag = { status: "over", count: event.count };
        this.projectDrop?.over(event.x, event.y, event.paths, event.count);
        return;
      case "leave":
        this.drag = { status: "idle" };
        this.projectDrop?.leave();
        return;
      case "drop": {
        this.drag = { status: "idle" };
        if (this.dropHook?.(event.paths)) return;
        if (this.projectDrop) {
          this.projectDrop.drop(event.paths, event.x, event.y);
          return;
        }
        const [first, ...rest] = event.paths;
        if (first === undefined || rest.length > 0) {
          this.#notify({ kind: "dropOne" });
          return;
        }
        void this.open(first, "ask");
        return;
      }
      default:
        event satisfies never;
    }
  }
}

export const windowStore = new WindowStore(api);

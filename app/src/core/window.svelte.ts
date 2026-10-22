// This view's state (one tab of a window, or the window's Welcome) + the open flow: boot, open folder | file |
// recent by placement (new tab | this tab | new window), close this tab, settings changes, notices. Backend
// owns the truth (`window-changed`); this mirrors it.

import {
  api,
  type Api,
  type ApiError,
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
import { confirmLeave } from "./editing/guard";
import { restartPlugins, syncPlugins } from "./plugins/instance.svelte";
import { language, parsePreference, t } from "./i18n";
import { look } from "./look.svelte";
import { tabs } from "./shell/tabs.svelte";
import { ui } from "./shell/ui.svelte";

export type NoticeAction = { label: string; run: () => void };

export type Notice =
  | { id: number; kind: "openFailed"; path: string; error: IpcError }
  | { id: number; kind: "apiFailed"; path: string | null; error: ApiError }
  | { id: number; kind: "stateRecovered"; notice: StateRecovered }
  | { id: number; kind: "settingNotSaved"; error: ApiError }
  | { id: number; kind: "initFailed"; path: string; error: ApiError }
  | { id: number; kind: "fileOpFailed"; error: ApiError }
  | { id: number; kind: "note"; text: string }
  /** A quiet line with buttons (Undo, Open in project); stays until dismissed. `onGone` hears how it went. */
  | {
      id: number;
      kind: "action";
      text: string;
      actions: readonly NoticeAction[];
      onGone?: (how: NoticeGone) => void;
    }
  /** A quiet line (buttons optional) that closes by itself after `closeAfterMs`, paused while held (pointer | focus
   *  on it); `onGone` hears how it went. */
  | {
      id: number;
      kind: "timed";
      text: string;
      actions: readonly NoticeAction[];
      closeAfterMs: number;
      onGone?: (how: NoticeGone) => void;
    };

/** How a notice went: its button ran, it was dismissed | timed out | pushed out by newer ones. */
export type NoticeGone = "action" | "dismissed" | "timeout" | "pushedOut";
/** Why a timed notice waits: the pointer | the focus is on it. */
export type NoticeHold = "pointer" | "focus";

/** The clock of a timed notice: time left, running since (`null`: held), who holds it. */
type NoticeClock = {
  left: number;
  since: number | null;
  handle: ReturnType<typeof setTimeout> | null;
  holds: Set<NoticeHold>;
};

type NoticeBody = Notice extends infer N ? (N extends Notice ? Omit<N, "id"> : never) : never;

export type Boot =
  { status: "booting" } | { status: "ready" } | { status: "failed"; error: ApiError };

export class WindowStore {
  readonly #api: Api;
  boot = $state.raw<Boot>({ status: "booting" });
  /** This view: events for it carry this label. */
  label = $state("");
  /** The native window it lives in. */
  window = $state("");
  /** Its tab (`null`: the window's Welcome view). */
  tab = $state<number | null>(null);
  view = $state.raw<WindowView>({ state: "welcome" });
  recent = $state.raw<readonly RecentFolder[]>([]);
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
  /** Where opening a folder | file goes: a new tab, this tab, a new window. */
  openIn = $state<OpenIn>("tab");
  /** The person's keyboard map: command id -> combo (`""` = unbound). */
  keymap = $state.raw<Readonly<Record<string, string>>>({});
  /** The tokenizers the token count shows, first = primary (the app's choice; a project may have its own). */
  tokenizers = $state.raw<readonly string[]>([]);
  #nextNotice = 1;
  readonly #clocks = new Map<number, NoticeClock>();

  constructor(backend: Api) {
    this.#api = backend;
  }

  /** Subscribes, then asks what this window shows (no event lost in between). Returns teardown of listeners. */
  async start(): Promise<Unlisten> {
    const offs = await Promise.all([
      this.#api.on("window-changed", (e) => {
        if (e.window !== this.label) return;
        // the Welcome view becomes the first tab's, and the last tab's view the Welcome again
        this.tab = e.tab;
        this.#showView(e.view);
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
        this.tokenizers = e.settings.tokenizers;
        void language.choose(parsePreference(e.settings.language));
      }),
      this.#api.on("open-failed", (e) => {
        if (e.window === this.label)
          this.#notify({ kind: "openFailed", path: e.path, error: e.error });
      }),
    ]);
    const booted = await this.#api.windowBoot();
    if (booted.ok) {
      const info = booted.value;
      this.label = info.label;
      this.window = info.window;
      this.tab = info.tab;
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
      this.tokenizers = info.settings.tokenizers;
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
    if (path) await this.open(path);
  }

  async openFile(): Promise<void> {
    const path = await this.#api.pickFile(t("dialog.openFile"), t("dialog.filter.docs"));
    if (path) await this.open(path);
  }

  /** No placement given: what the `open_in` setting says. */
  #placement(placement: OpenPlacement | undefined): OpenPlacement {
    if (placement !== undefined) return placement;
    switch (this.openIn) {
      case "tab":
        return "tab";
      case "this":
        return "thisTab";
      case "window":
        return "newWindow";
      default:
        return this.openIn satisfies never;
    }
  }

  /** Replacing what this tab shows lets go of its document first (its unsaved prompt). `false`: the person stays. */
  async #mayReplace(placement: OpenPlacement): Promise<boolean> {
    return placement !== "thisTab" || this.tab === null || confirmLeave({ kind: "workspace" });
  }

  /** Opens a folder | file: where it is already (one place), else by `placement` (default: the setting). */
  async open(path: string, placement?: OpenPlacement): Promise<void> {
    const where = this.#placement(placement);
    if (!(await this.#mayReplace(where))) return;
    await this.#opened(await this.#api.openPath(path, where), { kind: "path", path });
  }

  /** Opens the Notes: where they are already, else by `placement` (default: the setting). */
  async openNotes(placement?: OpenPlacement): Promise<void> {
    const where = this.#placement(placement);
    if (!(await this.#mayReplace(where))) return;
    await this.#opened(await this.#api.openNotes(where), { kind: "notes" });
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
      // another view shows it (a new tab, the tab it was in, another window): it says so itself
      case "newTab":
      case "focused":
      case "openedElsewhere":
        return;
      default:
        outcome satisfies never;
    }
  }

  /** Closes the active tab, the one on screen: this view's (its unsaved prompt runs first, asked by the backend). */
  async closeWorkspace(): Promise<void> {
    const tab = tabs.active ?? this.tab;
    if (tab !== null) await this.#api.tabClose(tab);
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

  /** New project: a folder is picked; an empty one is set up and opens like any folder. A folder with files in it
   *  comes back (`ask`): `.permark/` would go in among them, so the person is asked first. */
  async newProject(from?: string): Promise<{ ask: string } | null> {
    const path = await this.#api.pickFolder(t("dialog.newProject"), from);
    if (path === null) return null;
    const empty = await this.#api.folderEmpty(path);
    if (!empty.ok || !empty.value) return { ask: path };
    await this.createProject(path);
    return null;
  }

  /** PerMark is set up in the folder, which then opens. */
  async createProject(path: string): Promise<void> {
    if (await this.initProject(path)) await this.open(path);
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

  /** A quiet note ("Moved a.md", "Undone"); returns its id (`dismiss` takes it back). */
  notifyNote(text: string): number {
    return this.#notify({ kind: "note", text });
  }

  /** A quiet line with buttons ("Deleted a note" + Undo). */
  notifyAction(text: string, ...actions: NoticeAction[]): void {
    this.#notify({ kind: "action", text, actions });
  }

  /** A quiet line with buttons that stays until the person dismisses it (no clock: a keyboard user can reach it);
   *  `onGone` hears how it went. Returns its id (`dismiss` takes it back). */
  notifyLasting(body: {
    text: string;
    actions: readonly NoticeAction[];
    onGone: (how: NoticeGone) => void;
  }): number {
    return this.#notify({ kind: "action", ...body });
  }

  /** A quiet line that closes by itself (paused while held); `onGone` hears how it went. */
  notifyTimed(body: Omit<Extract<Notice, { kind: "timed" }>, "id" | "kind">): void {
    this.#notify({ kind: "timed", ...body });
  }

  /** A notice goes (`how`: its button | Dismiss | its time ran out). */
  dismiss(id: number, how: NoticeGone = "dismissed"): void {
    const gone = this.notices.find((n) => n.id === id);
    if (!gone) return;
    this.notices = this.notices.filter((n) => n.id !== id);
    this.#gone(gone, how);
  }

  /** The pointer | the focus is on a timed notice: its clock waits. */
  holdNotice(id: number, hold: NoticeHold): void {
    const clock = this.#clocks.get(id);
    if (!clock) return;
    clock.holds.add(hold);
    if (clock.since === null) return;
    if (clock.handle !== null) clearTimeout(clock.handle);
    clock.left = Math.max(0, clock.left - (Date.now() - clock.since));
    clock.since = null;
    clock.handle = null;
  }

  /** The pointer | the focus left a timed notice: its clock runs on when nothing holds it. */
  releaseNotice(id: number, hold: NoticeHold): void {
    const clock = this.#clocks.get(id);
    if (!clock) return;
    clock.holds.delete(hold);
    if (clock.holds.size === 0 && clock.since === null) this.#run(id, clock);
  }

  #run(id: number, clock: NoticeClock): void {
    clock.since = Date.now();
    clock.handle = setTimeout(() => this.dismiss(id, "timeout"), clock.left);
  }

  #gone(notice: Notice, how: NoticeGone): void {
    const clock = this.#clocks.get(notice.id);
    if (clock?.handle) clearTimeout(clock.handle);
    this.#clocks.delete(notice.id);
    if (notice.kind === "timed" || notice.kind === "action") notice.onGone?.(how);
  }

  #notify(body: NoticeBody): number {
    const notice = { ...body, id: this.#nextNotice++ } as Notice;
    // newest last; at most 5 on screen
    const all = [...this.notices, notice];
    const out = all.slice(0, Math.max(0, all.length - 5));
    this.notices = all.slice(-5);
    for (const old of out) this.#gone(old, "pushedOut");
    if (notice.kind === "timed") {
      const clock: NoticeClock = {
        left: notice.closeAfterMs,
        since: null,
        handle: null,
        holds: new Set(),
      };
      this.#clocks.set(notice.id, clock);
      this.#run(notice.id, clock);
    }
    return notice.id;
  }
}

export const windowStore = new WindowStore(api);

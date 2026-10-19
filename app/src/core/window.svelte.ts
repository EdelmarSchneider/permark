// This window's state + the open flow: boot, open folder | file | recent | drop, "This window | New window",
// close back to Welcome, settings changes, notices. Backend owns the truth (`window-changed`); this mirrors it.

import {
  api,
  type Api,
  type ApiError,
  type DropEvent,
  type IpcError,
  type LinkUpdate,
  type OpenPlacement,
  type RecentFolder,
  type SettingChange,
  type StateRecovered,
  type Unlisten,
  type WindowView,
} from "../api";
import { conditions } from "./conditions/context.svelte";
import { syncPlugins } from "./plugins/instance.svelte";
import { language, parsePreference, t } from "./i18n";
import { look } from "./look.svelte";

export type Notice =
  | { id: number; kind: "openFailed"; path: string; error: IpcError }
  | { id: number; kind: "apiFailed"; path: string | null; error: ApiError }
  | { id: number; kind: "dropOne" }
  | { id: number; kind: "stateRecovered"; notice: StateRecovered }
  | { id: number; kind: "settingNotSaved"; error: ApiError }
  | { id: number; kind: "fileOpFailed"; error: ApiError }
  | { id: number; kind: "note"; text: string };

type NoticeBody = Notice extends infer N ? (N extends Notice ? Omit<N, "id"> : never) : never;

export type Choice = { status: "closed" } | { status: "asking"; path: string };
export type Drag = { status: "idle" } | { status: "over"; count: number };
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
  /** The command that opens a file in the person's editor (setting; empty = not set). */
  editor = $state("");
  #nextNotice = 1;

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
        this.editor = e.settings.editor;
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
      this.editor = info.settings.editor;
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
    this.view = view;
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

  async open(path: string, placement: OpenPlacement): Promise<void> {
    const result = await this.#api.openPath(path, placement);
    if (!result.ok) {
      // a newer open in this window won: nothing to tell
      if (result.error.kind === "backend" && result.error.error.kind === "superseded") return;
      this.#notify({ kind: "apiFailed", path, error: result.error });
      return;
    }
    const outcome = result.value;
    switch (outcome.outcome) {
      case "opened":
        this.#showView(outcome.view);
        return;
      case "needsChoice":
        this.choice = { status: "asking", path };
        return;
      case "openedElsewhere":
        return;
      default:
        outcome satisfies never;
    }
  }

  /** Answer of the "This window | New window" dialog; `null` = cancelled. */
  async answerChoice(answer: "here" | "new" | null): Promise<void> {
    const asked = this.choice;
    this.choice = { status: "closed" };
    if (asked.status !== "asking" || answer === null) return;
    await this.open(asked.path, answer === "here" ? "thisWindow" : "newWindow");
  }

  async closeWorkspace(): Promise<void> {
    await this.#api.showWelcome();
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

  /** A file operation of the Explorer failed. */
  notifyFileOpFailed(error: ApiError): void {
    this.#notify({ kind: "fileOpFailed", error });
  }

  /** A quiet note ("Moved a.md", "Undone"). */
  notifyNote(text: string): void {
    this.#notify({ kind: "note", text });
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
        return;
      case "leave":
        this.drag = { status: "idle" };
        return;
      case "drop": {
        this.drag = { status: "idle" };
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

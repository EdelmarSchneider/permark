// The token count as this window sees it: the vocabularies (the backend announces them, one plugin for the whole app),
// the tokenizers chosen (the app's list, or the project's own over it), and the person's actions. Counting itself
// lives in `figures.svelte.ts` (it follows the text on screen).

import {
  api as defaultApi,
  type Api,
  type SettingChange,
  type ProjectSettings,
  type TokenCountState,
  type VocabEntry,
} from "../../api";
import { pluginClient, type PluginClient } from "../../core/plugins/client";
import { projectSettings } from "../../core/shell/settings/stores.svelte";
import { windowStore } from "../../core/window.svelte";
import { effective, infoOf, type TokenizerId, type TokenizerInfo } from "./tokenizers";

/** Where a choice is kept: for every project, or in `.permark/config.toml` of this one. */
export type Scope = "app" | "project";

export class TokenStore {
  /** The vocabularies and the outline switch, as the backend announces them (`null` until first told). */
  state = $state.raw<TokenCountState | null>(null);
  /** The status item's popover is open. */
  popover = $state(false);
  /** The popover shows the chooser. */
  choosing = $state(false);
  readonly #api: Api;
  readonly #client: PluginClient;
  /** The project's shared settings as first read (the live ones are `projectSettings` once the settings page loaded). */
  #loaded = $state.raw<ProjectSettings | null>(null);

  constructor(api: Api = defaultApi, client: PluginClient = pluginClient) {
    this.#api = api;
    this.#client = client;
  }

  /** Reads the state and follows it; returns the teardown. */
  start(): () => void {
    let gone = false;
    let off: (() => void) | null = null;
    void this.#client.call("token-count.state", {}).then((result) => {
      if (!gone && result.ok) this.state = result.value;
    });
    void this.#api.projectConfigGet().then((result) => {
      if (!gone && result.ok) this.#loaded = result.value;
    });
    void this.#api
      .onAppPlugin("token-count:state", (state) => {
        this.state = state;
      })
      .then(
        (stop) => {
          if (gone) stop();
          else off = stop;
        },
        () => {
          // no backend to listen to (a test, a browser): the state read above is all there is
        },
      );
    return () => {
      gone = true;
      off?.();
      this.popover = false;
      this.choosing = false;
      this.state = null;
      this.#loaded = null;
    };
  }

  /** The app's choice. */
  get appList(): readonly string[] {
    return windowStore.tokenizers;
  }

  /** The project's shared settings: the live ones once the settings page loaded them, else as first read. */
  get #project(): ProjectSettings | null {
    const settings = projectSettings.state;
    return settings.status === "ready" ? settings.settings : this.#loaded;
  }

  /** The project's own choice (empty = none: the app's applies). */
  get projectList(): readonly string[] {
    return this.#project?.tokenizers ?? [];
  }

  /** This project can keep a choice of its own: it has `.permark/`. */
  get projectCanChoose(): boolean {
    return this.#project?.initialized ?? false;
  }

  /** The tokenizers in use, first = primary. */
  get chosen(): TokenizerInfo[] {
    return effective(this.appList, this.projectList);
  }

  get primary(): TokenizerInfo | null {
    return this.chosen[0] ?? null;
  }

  /** The vocabulary of an exact tokenizer. */
  vocabOf(id: TokenizerId): VocabEntry | null {
    if (this.state === null) return null;
    if (id === "o200k") return this.state.vocab.o200k;
    if (id === "cl100k") return this.state.vocab.cl100k;
    return null;
  }

  /** The tokenizer can give a number now: an estimate, or an exact one with its vocabulary on this computer. */
  ready(info: TokenizerInfo): boolean {
    if (!info.exact) return true;
    return this.vocabOf(info.id)?.state.kind === "ready";
  }

  /** Keeps a choice: for every project (`app`) or for this one (`project`). `true` when it was kept. */
  async choose(scope: Scope, ids: readonly string[]): Promise<boolean> {
    const list = [...ids];
    if (scope === "app") {
      const change: SettingChange = { key: "tokenizers", value: list };
      const refused = await windowStore.trySetting(change);
      return refused === null;
    }
    return projectSettings.set({ key: "tokenizers", value: list });
  }

  async download(id: TokenizerId): Promise<void> {
    await this.#send("token-count.download", id);
  }

  async cancel(id: TokenizerId): Promise<void> {
    await this.#send("token-count.cancel", id);
  }

  async remove(id: TokenizerId): Promise<void> {
    await this.#send("token-count.removeVocab", id);
  }

  async #send(
    method: "token-count.download" | "token-count.cancel" | "token-count.removeVocab",
    id: TokenizerId,
  ): Promise<void> {
    if (infoOf(id) === null) return;
    const result = await this.#client.call(method, { tokenizer: id });
    if (result.ok) this.state = result.value;
  }

  async setOutline(on: boolean): Promise<void> {
    const result = await this.#client.call("token-count.setOutline", { on });
    if (result.ok) this.state = result.value;
  }

  /** Whether the folders the person opens show their total (small ones). */
  async setFolderTotals(on: boolean): Promise<void> {
    const result = await this.#client.call("token-count.setFolderTotals", { on });
    if (result.ok) this.state = result.value;
  }

  /** The popover opens on the chooser (nothing chosen yet, or the person asked to change). */
  openChooser(): void {
    this.choosing = true;
    this.popover = true;
  }
}

export const tokens = new TokenStore();

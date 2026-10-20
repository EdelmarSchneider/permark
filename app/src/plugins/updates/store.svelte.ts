// The updater as this window sees it: the state the backend announces (`updates:state`, one updater for the whole
// app), the chip's popover, and the person's actions. A restart is refused while any window holds unsaved edits.

import { api as defaultApi, type Api, type UpdateState } from "../../api";
import { apiErrorText } from "../../core/errors";
import { t } from "../../core/i18n";
import { pluginClient, type PluginClient } from "../../core/plugins/client";
import { windowStore } from "../../core/window.svelte";
import { statusLine, type Action } from "./model";

export class UpdatesStore {
  state = $state.raw<UpdateState>({ state: "idle" });
  popover = $state(false);
  readonly #api: Api;
  readonly #client: PluginClient;
  /** A check asked from the palette: its answer is told as a notice (no chip says "up to date"). */
  #tell = false;

  constructor(api: Api = defaultApi, client: PluginClient = pluginClient) {
    this.#api = api;
    this.#client = client;
  }

  /** Reads the state and follows it; returns the teardown. */
  start(): () => void {
    let gone = false;
    let off: (() => void) | null = null;
    void this.#client.call("updates.state", {}).then((result) => {
      if (!gone && result.ok) this.state = result.value;
    });
    void this.#api
      .onAppPlugin("updates:state", (state) => {
        this.state = state;
        this.#told(state);
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
    };
  }

  async run(action: Action): Promise<void> {
    switch (action) {
      case "check": {
        const result = await this.#client.call("updates.check", {});
        if (result.ok) this.state = result.value;
        return;
      }
      case "download": {
        const result = await this.#client.call("updates.download", {});
        if (result.ok) this.state = result.value;
        return;
      }
      case "restart": {
        const result = await this.#client.call("updates.restart", {});
        if (result.ok) return;
        const error = result.error;
        if (error.kind === "backend" && error.error.kind === "conflict") {
          const files = (error.error.subject ?? "").split("\n").filter((f) => f !== "");
          windowStore.notifyNote(t("updates.unsaved", { files: files.join(", ") }));
        } else {
          windowStore.notifyNote(apiErrorText(error));
        }
        return;
      }
      default:
        action satisfies never;
    }
  }

  /** Checks and tells the answer as a notice (the palette's "Check for updates"). */
  async checkAndTell(): Promise<void> {
    this.#tell = true;
    await this.run("check");
    // nothing started (a download or a ready update): say where the updater stands
    if (this.state.state !== "checking") this.#told(this.state);
  }

  #told(state: UpdateState): void {
    if (!this.#tell || state.state === "checking") return;
    this.#tell = false;
    windowStore.notifyNote(statusLine(state));
  }

  /** "Later": the popover closes; an update stays offered, a failure is put away. */
  later(): void {
    this.popover = false;
    if (this.state.state === "error") void this.dismiss();
  }

  /** The person saw "up to date" | the error. */
  async dismiss(): Promise<void> {
    const result = await this.#client.call("updates.dismiss", {});
    if (result.ok) this.state = result.value;
  }
}

export const updates = new UpdatesStore();

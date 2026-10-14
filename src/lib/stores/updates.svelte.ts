// The app updater: a check a few seconds after launch (release builds only), a manual check from the
// settings, and "Restart to update". The state machine is in `../updates`; this store runs the steps.

import { errorMessage } from "../errors";
import { t } from "../i18n";
import { checkForUpdate, isMock, type AvailableUpdate } from "../platform";
import {
  INITIAL_UPDATE_STATE,
  reduceUpdate,
  type DownloadProgress,
  type UpdateEvent,
  type UpdateState,
} from "../updates";
import { edit } from "./edit.svelte";
import { toast } from "./toast.svelte";

/** Let the app finish starting (and the network settle) before asking the update server. */
const STARTUP_CHECK_DELAY_MS = 5000;

class UpdatesStore {
  state = $state.raw<UpdateState>(INITIAL_UPDATE_STATE);

  private found: AvailableUpdate | null = null;

  /** Checks once, shortly after launch. Release builds only, so `tauri dev` and the browser mock stay quiet. */
  scheduleStartupCheck(): void {
    if (!import.meta.env.PROD || isMock) return;
    setTimeout(() => void this.check(false), STARTUP_CHECK_DELAY_MS);
  }

  /**
   * Asks the update server. An automatic check ignores errors and "no update"; a manual one reports both.
   * When an update is known already, a manual check just shows the offer again.
   */
  async check(manual: boolean): Promise<void> {
    if (this.state.phase === "available") {
      if (manual) toast.update();
      return;
    }
    this.dispatch({ type: "checkStarted", manual });
    if (this.state.phase !== "checking") return; // another check or an install is running
    try {
      const update = await checkForUpdate();
      if (!update) {
        this.dispatch({ type: "none" });
        return;
      }
      this.found = update;
      this.dispatch({ type: "found", version: update.version });
      toast.update();
    } catch (error) {
      this.dispatch({ type: "checkFailed", message: errorMessage(error) });
    }
  }

  /** Brings the "Update available" toast back (the status bar item does this). */
  announce(): void {
    if (this.state.phase === "available") toast.update();
    else if (this.state.phase === "downloading" || this.state.phase === "installing") {
      toast.update(true);
    }
  }

  /**
   * "Restart to update": downloads and installs the update, then restarts the app. The restart (and, on Windows, the
   * installer) closes the window without a close request, so unsaved edits are asked about first.
   */
  async install(): Promise<void> {
    const update = this.found;
    if (!update || this.state.phase !== "available") return;
    if (!(await edit.confirmLeave("restart"))) return;
    if (this.state.phase !== "available") return; // an install started while the question was open
    this.dispatch({ type: "installRequested" });
    toast.update(true);
    try {
      await update.install((progress) => this.onProgress(progress));
    } catch (error) {
      const message = errorMessage(error);
      this.dispatch({ type: "installFailed", message });
      toast.message(t("update.installFailed", { message }), "warn");
    }
  }

  /** The settings popover closed: forget "You're up to date" and a failed check. */
  dismissResult(): void {
    this.dispatch({ type: "dismissResult" });
  }

  private onProgress(progress: DownloadProgress): void {
    switch (progress.kind) {
      case "started":
        this.dispatch({ type: "downloadStarted", total: progress.total });
        break;
      case "chunk":
        this.dispatch({ type: "downloadProgress", chunk: progress.bytes });
        break;
      case "finished":
        this.dispatch({ type: "downloadFinished" });
        break;
    }
  }

  private dispatch(event: UpdateEvent): void {
    this.state = reduceUpdate(this.state, event);
  }
}

export const updates = new UpdatesStore();

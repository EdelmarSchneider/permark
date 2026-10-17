// "Open with PerMark" in the file manager: whether the right-click entry and the file association are registered, as the
// system has them right now (the person, the installer or another program may have changed them), and the switches in Settings.

import { api } from "../api";
import { errorMessage } from "../errors";
import { language } from "../i18n";
import type { ShellState } from "../types";
import { toast } from "./toast.svelte";

class ShellStore {
  /** Null until read, and when it cannot be (the section stays hidden). */
  state = $state.raw<ShellState | null>(null);
  /** A change is being written: the switches wait for it. */
  busy = $state(false);

  /** Reads what is registered. Never rejects. */
  async load(): Promise<void> {
    try {
      this.state = await api.shellIntegrationState();
    } catch {
      this.state = null;
    }
  }

  async setContextMenu(enabled: boolean): Promise<void> {
    await this.change(() => api.setShellContextMenu(enabled, language.locale));
  }

  async setAssociation(enabled: boolean): Promise<void> {
    await this.change(() => api.setShellFileAssociation(enabled, language.locale));
  }

  /** Opens the page of Windows Settings where the default app is chosen (Windows does not let a program pick itself). */
  async makeDefault(): Promise<void> {
    try {
      await api.openDefaultApps();
    } catch (error) {
      toast.message(errorMessage(error), "warn");
    }
  }

  private async change(run: () => Promise<ShellState>): Promise<void> {
    this.busy = true;
    try {
      this.state = await run();
    } catch (error) {
      toast.message(errorMessage(error), "warn");
      await this.load();
    } finally {
      this.busy = false;
    }
  }
}

export const shell = new ShellStore();

/**
 * Rewords the entries that exist in the language now on screen. Best effort, with no message: the entries still work
 * in the language they were written in.
 */
export async function rewordShellEntries(): Promise<void> {
  try {
    await api.refreshShellLabels(language.locale);
  } catch {
    // nothing to tell: the old wording stays
  }
}

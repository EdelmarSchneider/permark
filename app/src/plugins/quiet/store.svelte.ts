// Silence: the marks live in the backend (`quiet.json`); this mirrors them into the core `silence` store the plugins that
// call for attention ask, and sends the person's choices (Silence | Listen) back.

import { t } from "../../core/i18n";
import { PluginClient, pluginClient, workspaceRoot } from "../../core/plugins/client";
import type { TreeBadge } from "../../core/plugins/registry.svelte";
import { silence, type Interest } from "../../core/silence/silence.svelte";

export class QuietStore {
  readonly #client: PluginClient;
  readonly #root: () => string | null;

  constructor(client: PluginClient = pluginClient, root: () => string | null = workspaceRoot) {
    this.#client = client;
    this.#root = root;
  }

  /** Follows the project of the window until the returned teardown runs. */
  start(): () => void {
    return $effect.root(() => {
      $effect(() => {
        if (this.#root() === null) {
          silence.clear();
          return;
        }
        let off: (() => void) | null = null;
        let gone = false;
        void this.#client
          .on("quiet:changed", (data) => silence.set(data.marks))
          .then((stop) => {
            if (gone) stop();
            else off = stop;
          });
        void this.#client.call("quiet.rules", {}).then((result) => {
          if (!gone && result.ok) silence.set(result.value.marks);
        });
        return () => {
          gone = true;
          off?.();
        };
      });
      return () => silence.clear();
    });
  }

  /** Silences | listens to these project paths; the answer is every mark now. */
  async set(paths: readonly string[], interest: Interest): Promise<void> {
    const result = await this.#client.call("quiet.set", { paths: [...paths], interest });
    if (result.ok) silence.set(result.value.marks);
  }

  /** A quieter row with a word in its tooltip. */
  badge(path: string): TreeBadge | null {
    return silence.isSilenced(path) ? { text: "", dim: true, tip: t("quiet.mark") } : null;
  }
}

export const quiet = new QuietStore();

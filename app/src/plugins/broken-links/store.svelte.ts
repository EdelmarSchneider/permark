// Broken links: the list the backend keeps true (a worker follows the project's changes). This mirrors it for the tab.

import type { BrokenReport } from "../../api";
import { PluginClient, pluginClient, workspaceRoot } from "../../core/plugins/client";

/** The events of a burst become one reload. */
export const RELOAD_MS = 250;

export class BrokenLinksStore {
  report = $state.raw<BrokenReport | null>(null);
  readonly #client: PluginClient;
  readonly #root: () => string | null;
  #timer: ReturnType<typeof setTimeout> | null = null;

  constructor(client: PluginClient = pluginClient, root: () => string | null = workspaceRoot) {
    this.#client = client;
    this.#root = root;
  }

  /** How many links are broken (0 until the first scan has been told). */
  get total(): number {
    return this.report?.total ?? 0;
  }

  /** Follows the project of the window until the returned teardown runs. */
  start(): () => void {
    return $effect.root(() => {
      $effect(() => {
        if (this.#root() === null) {
          this.report = null;
          return;
        }
        let off: (() => void) | null = null;
        let gone = false;
        void this.#client
          .on("broken-links:changed", () => this.#schedule())
          .then((stop) => {
            if (gone) stop();
            else off = stop;
          });
        void this.load();
        return () => {
          gone = true;
          off?.();
          this.#cancel();
        };
      });
      return () => {
        this.#cancel();
        this.report = null;
      };
    });
  }

  #cancel(): void {
    if (this.#timer !== null) clearTimeout(this.#timer);
    this.#timer = null;
  }

  #schedule(): void {
    if (this.#timer !== null) return;
    this.#timer = setTimeout(() => {
      this.#timer = null;
      void this.load();
    }, RELOAD_MS);
  }

  async load(): Promise<void> {
    const root = this.#root();
    const result = await this.#client.call("broken-links.list", {});
    if (this.#root() === root && result.ok) this.report = result.value;
  }
}

export const brokenLinks = new BrokenLinksStore();

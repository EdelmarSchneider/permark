// How a plugin's UI talks to its backend half: typed calls of `<plugin>.<method>` and the events of the project this
// window shows. Nothing here knows a plugin; the typed maps (`PluginMethods`, `PluginEvents`) are in `api/types.ts`.

import {
  api as defaultApi,
  type Api,
  type PluginEnvelope,
  type PluginEventName,
  type PluginMethod,
  type PluginMethods,
  type Result,
  type Unlisten,
} from "../../api";
import { windowStore } from "../window.svelte";

/** The folder of the project this window shows (`null`: none). Reactive. */
export function workspaceRoot(): string | null {
  const view = windowStore.view;
  switch (view.state) {
    case "showing":
      return view.workspace.kind === "project" ? view.workspace.root : null;
    case "opening":
      return view.showing?.kind === "project" ? view.showing.root : null;
    case "welcome":
    case "closing":
      return null;
    default:
      return view satisfies never;
  }
}

export class PluginClient {
  readonly #api: Api;
  readonly #root: () => string | null;

  constructor(api: Api = defaultApi, root: () => string | null = workspaceRoot) {
    this.#api = api;
    this.#root = root;
  }

  call<M extends PluginMethod>(
    method: M,
    args: PluginMethods[M]["args"],
  ): Promise<Result<PluginMethods[M]["result"]>> {
    return this.#api.pluginCall(method, args);
  }

  /** Hears an event about the project this window shows (other projects' events are not passed on). */
  on<E extends PluginEventName>(
    event: E,
    handler: (data: PluginEnvelope<E>["data"]) => void,
  ): Promise<Unlisten> {
    return this.#api.onPlugin(event, (payload) => {
      if (payload.root === this.#root()) handler(payload.data);
    });
  }
}

/** The client of this window. */
export const pluginClient = new PluginClient();

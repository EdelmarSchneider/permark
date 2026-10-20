// What the distribution parts answer in the mock backend: the default app (Windows hand-off by default), the file
// manager entries and the updater. In memory; the same shapes as the real commands and plugins.

import type { DefaultAppMode } from "./generated/DefaultAppMode";
import { MockPluginError } from "./mockOrganizing";
import type {
  AppPluginEventName,
  AppPluginEvents,
  DefaultAppView,
  FileGroup,
  PluginMethods,
  Result,
  ShellState,
  UpdateState,
} from "./types";

const GROUPS: { group: FileGroup; extensions: string[] }[] = [
  { group: "markdown", extensions: [".md", ".markdown", ".mdown", ".mkd"] },
  { group: "text", extensions: [".txt"] },
  { group: "json", extensions: [".json", ".jsonl"] },
  { group: "xml", extensions: [".xml"] },
  { group: "yaml", extensions: [".yaml", ".yml"] },
  { group: "toml", extensions: [".toml"] },
  { group: "ini", extensions: [".ini", ".cfg"] },
  { group: "properties", extensions: [".properties"] },
];

type DistributionMethods = Pick<
  PluginMethods,
  | "shell-integration.state"
  | "shell-integration.set"
  | "updates.state"
  | "updates.check"
  | "updates.download"
  | "updates.restart"
  | "updates.dismiss"
>;

export type DistributionProbe = {
  /** How this copy can set the default app, and whether the first-run question is due. */
  setDefaultApp(mode: DefaultAppMode, ask: boolean): void;
  /** Puts the updater in a state (as the backend would announce it). */
  setUpdate(state: UpdateState): void;
  /** What a check finds next (`null`: nothing newer). */
  offer: string | null;
  /** Documents with unsaved edits (a restart is refused while there are any). */
  unsaved: string[];
  /** Restarts asked for. */
  restarts: number;
  handoffs: number;
};

const ok = <T>(value: T): Promise<Result<T>> => Promise.resolve({ ok: true, value });

export function createMockDistribution(
  emit: <E extends AppPluginEventName>(event: E, payload: AppPluginEvents[E]) => void,
) {
  let mode: DefaultAppMode = { mode: "handoff" };
  let ask = false;
  const ours = new Set<FileGroup>(["markdown"]);
  let shell: ShellState = { menu: true, newFile: false };
  let update: UpdateState = { state: "idle" };

  const probe: DistributionProbe = {
    setDefaultApp: (next, nextAsk) => {
      mode = next;
      ask = nextAsk;
    },
    setUpdate: (next) => setUpdate(next),
    offer: "0.7.0",
    unsaved: [],
    restarts: 0,
    handoffs: 0,
  };

  function setUpdate(next: UpdateState): void {
    update = next;
    emit("updates:state", next);
  }

  function view(): DefaultAppView {
    return {
      mode,
      ask,
      groups: GROUPS.map((g) => ({
        group: g.group,
        extensions: g.extensions,
        ours: mode.mode !== "unsupported" && ours.has(g.group),
      })),
    };
  }

  const handlers: {
    [M in keyof DistributionMethods]: (
      args: DistributionMethods[M]["args"],
    ) => DistributionMethods[M]["result"];
  } = {
    "shell-integration.state": () => shell,
    "shell-integration.set": (args) => {
      shell = args.entry === "menu" ? { ...shell, menu: args.on } : { ...shell, newFile: args.on };
      return shell;
    },
    "updates.state": () => update,
    "updates.check": () => {
      if (update.state === "idle" || update.state === "upToDate" || update.state === "error") {
        setUpdate({ state: "checking", manual: true });
        const offer = probe.offer;
        setTimeout(() => {
          setUpdate(
            offer === null
              ? { state: "upToDate" }
              : { state: "available", version: offer, notes: null },
          );
        }, 300);
      }
      return update;
    },
    "updates.download": () => {
      if (update.state !== "available") return update;
      const version = update.version;
      setUpdate({ state: "downloading", version, percent: 0 });
      let percent = 0;
      const step = setInterval(() => {
        percent += 25;
        if (percent >= 100) {
          clearInterval(step);
          setUpdate({ state: "ready", version });
        } else setUpdate({ state: "downloading", version, percent });
      }, 150);
      return update;
    },
    "updates.restart": () => {
      if (update.state !== "ready") throw new MockPluginError("unavailable", "update");
      if (probe.unsaved.length > 0) throw new MockPluginError("conflict", probe.unsaved.join("\n"));
      probe.restarts += 1;
      return update;
    },
    "updates.dismiss": () => {
      if (update.state === "upToDate" || update.state === "error") setUpdate({ state: "idle" });
      return update;
    },
  };

  return {
    handlers,
    probe,
    defaultAppGet: () => ok(view()),
    defaultAppSet: (group: FileGroup, on: boolean): Promise<Result<DefaultAppView>> => {
      if (mode.mode !== "direct") {
        return Promise.resolve({
          ok: false,
          error: {
            kind: "backend",
            command: "default_app_set",
            error: { kind: "unsupported", subject: "defaultApp", detail: null },
          },
        });
      }
      if (on) ours.add(group);
      else ours.delete(group);
      ask = false;
      return ok(view());
    },
    defaultAppAsked: () => {
      ask = false;
      return ok(null);
    },
    defaultAppHandoff: () => {
      probe.handoffs += 1;
      ask = false;
      return ok(null);
    },
  };
}

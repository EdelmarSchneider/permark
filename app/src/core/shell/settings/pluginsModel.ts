// The Plugins page's rules, pure: the plugins grouped by kind, and what a row says about a plugin that cannot work here.

import type { PluginInfo, PluginKind } from "../../../api";
import { available, type Context, type Reason } from "../../conditions/available";
import type { MessageKey } from "../../i18n";

/** Every plugin of the full build (a name + a line each in the locale files). */
export const KNOWN_PLUGINS = [
  "git",
  "live-feed",
  "follow",
  "unread",
  "review",
  "tags",
  "quiet",
  "comments",
  "broken-links",
  "notes",
  "agent",
  "md-family",
  "word-count",
  "prompt-blocks",
  "frontmatter-card",
  "highlight",
  "math",
  "mermaid",
  "plantuml",
  "media",
  "json-view",
  "xml-view",
  "fmt-toml",
  "fmt-yaml",
  "fmt-ini",
  "fmt-dotenv",
  "fmt-properties",
  "fmt-ignore",
  "text-view",
  "shell-integration",
  "updates",
] as const;
type Known = (typeof KNOWN_PLUGINS)[number];

function known(id: string): Known | null {
  return KNOWN_PLUGINS.find((k) => k === id) ?? null;
}

/** The name | one-line description of a plugin in the person's language (`null`: a plugin the locale files lack). */
export function nameKey(id: string): MessageKey | null {
  const found = known(id);
  return found === null ? null : `plugin.${found}.name`;
}
export function aboutKey(id: string): MessageKey | null {
  const found = known(id);
  return found === null ? null : `plugin.${found}.about`;
}

/** Kinds in the order the page lists them. */
export const KIND_ORDER: readonly PluginKind[] = [
  "feature",
  "format",
  "role",
  "analysis",
  "checker",
  "embed",
];

export function kindKey(kind: PluginKind): MessageKey {
  switch (kind) {
    case "feature":
      return "plugins.kind.feature";
    case "format":
      return "plugins.kind.format";
    case "role":
      return "plugins.kind.role";
    case "analysis":
      return "plugins.kind.analysis";
    case "checker":
      return "plugins.kind.checker";
    case "embed":
      return "plugins.kind.embed";
    default:
      return kind satisfies never;
  }
}

/** What a row says besides the switch. */
export type Status =
  | { kind: "none" }
  /** On, but this window lacks something the plugin needs. */
  | { kind: "needs"; reason: Reason }
  /** Off because a plugin it needs is off. */
  | { kind: "dependency"; plugin: string }
  /** Off in this project (the person's choice for it | the project's own settings). */
  | { kind: "project" };

export function statusOf(info: PluginInfo, context: Context): Status {
  const state = info.state;
  switch (state.state) {
    case "on": {
      const found = available(info.manifest.requires, context);
      return found.status === "unavailable"
        ? { kind: "needs", reason: found.reason }
        : { kind: "none" };
    }
    case "off":
      switch (state.why.reason) {
        case "dependency":
          return { kind: "dependency", plugin: state.why.plugin };
        case "project":
          return { kind: "project" };
        case "setting":
          return { kind: "none" };
        default:
          return state.why satisfies never;
      }
    default:
      return state satisfies never;
  }
}

export type Group = { kind: PluginKind; plugins: PluginInfo[] };

/** The plugins by kind in [`KIND_ORDER`]; manifest order inside a group; groups with no plugin are left out. */
export function grouped(infos: readonly PluginInfo[]): Group[] {
  return KIND_ORDER.flatMap((kind) => {
    const plugins = infos.filter((info) => info.manifest.kind === kind);
    return plugins.length === 0 ? [] : [{ kind, plugins }];
  });
}

/** "Remove data" is offered while the plugin is off at the app level (the backend refuses while one still runs). */
export function canRemoveData(info: PluginInfo): boolean {
  return !info.appOn && info.state.state === "off";
}

/** The choice a per-project select shows. */
export type ProjectChoice = "app" | "on" | "off";

export function choiceOf(info: PluginInfo): ProjectChoice {
  return info.projectChoice === null ? "app" : info.projectChoice ? "on" : "off";
}

export function choiceValue(choice: ProjectChoice): boolean | null {
  switch (choice) {
    case "app":
      return null;
    case "on":
      return true;
    case "off":
      return false;
    default:
      return choice satisfies never;
  }
}

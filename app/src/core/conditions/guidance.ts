// Guidance: a reason in one plain sentence + the label of the command that fixes it. Shown where the person reaches
// for the feature (disabled command tooltip, palette note, empty panel), never as a banner.

import type { MessageKey } from "../i18n";
import type { Reason } from "./available";

export function reasonKey(reason: Reason): MessageKey {
  switch (reason.need) {
    case "workspace":
      // a feature for projects only says what to open; one for documents may be satisfied by a file too
      return reason.kinds.length === 1 && reason.kinds[0] === "project"
        ? "condition.workspaceProject"
        : "condition.workspace";
    case "gitRepo":
      return "condition.gitRepo";
    case "gitAvailable":
      return "condition.gitAvailable";
    case "permark":
      return "condition.permark";
    case "agentOn":
      return "condition.agentOn";
    case "agentConnected":
      return "condition.agentConnected";
    case "liveUpdates":
      return "condition.liveUpdates";
    case "distribution":
      return "condition.distribution";
    case "platform":
      return "condition.platform";
    case "plugin":
      return "condition.plugin";
    default:
      return reason satisfies never;
  }
}

/** Label key of a remedy command (`plugins.enable:<id>` -> `plugins.enable`); unknown command -> null. */
export function remedyKey(command: string | null): MessageKey | null {
  switch (command?.split(":")[0]) {
    case "project.init":
      return "condition.remedy.project.init";
    case "agent.turnOn":
      return "condition.remedy.agent.turnOn";
    case "plugins.enable":
      return "condition.remedy.plugins.enable";
    case "settings.git":
      return "condition.remedy.settings.git";
    default:
      return null;
  }
}

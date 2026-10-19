// TS mirror of `permark-core::conditions::available` (same algorithm; shared fixture `fixture.json` keeps them equal).

import type { Availability } from "../../api/generated/Availability";
import type { Context } from "../../api/generated/Context";
import type { Need } from "../../api/generated/Need";
import type { Reason } from "../../api/generated/Reason";
import type { Requirement } from "../../api/generated/Requirement";

export type { Availability, Context, Need, Reason, Requirement };

export const NO_REQUIREMENT: Requirement = { needs: [], prefers: [] };

export function met(need: Need, context: Context): boolean {
  switch (need.need) {
    case "workspace":
      return need.kinds.includes(context.workspace);
    case "gitRepo":
      return context.git.status === "repo";
    case "gitAvailable":
      return context.git.status !== "unavailable";
    case "permark":
      return context.permark === "initialized";
    case "agentOn":
      return context.agent.status === "on" || context.agent.status === "connected";
    case "agentConnected":
      return context.agent.status === "connected";
    case "liveUpdates":
      return context.liveUpdates.status === "available";
    case "distribution":
      return context.distribution === need.distribution;
    case "platform":
      return need.platforms.includes(context.platform);
    case "plugin":
      return context.plugins.includes(need.plugin);
    default:
      return need satisfies never;
  }
}

export function remedy(need: Need): string | null {
  switch (need.need) {
    case "permark":
      return "project.init";
    case "agentOn":
      return "agent.turnOn";
    case "plugin":
      return `plugins.enable:${need.plugin}`;
    case "gitAvailable":
      return "settings.git";
    case "workspace":
    case "gitRepo":
    case "agentConnected":
    case "liveUpdates":
    case "distribution":
    case "platform":
      return null;
    default:
      return need satisfies never;
  }
}

function reason(need: Need): Reason {
  return { ...need, remedy: remedy(need) };
}

/** Availability of a requirement in a context. Pure. */
export function available(requirement: Requirement, context: Context): Availability {
  const unmet = requirement.needs.find((need) => !met(need, context));
  if (unmet) return { status: "unavailable", reason: reason(unmet) };
  return {
    status: "available",
    degraded: requirement.prefers.filter((need) => !met(need, context)).map(reason),
  };
}

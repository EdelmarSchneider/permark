// A project that calls for agents: a git repository, or agent files at its root (`conditions.md` `agentFiles`). Pure;
// read by the live feed (shown by default there) and the agent-share offer. Plugin `agent` off still counts.

import type { Context } from "../../api";

/** The agent files the backend found at the project root (`[]` outside a project | before the fact is known). */
export function agentFilesOf(context: Context): readonly string[] {
  const files = (context as Context & { agentFiles?: unknown }).agentFiles;
  return Array.isArray(files) ? files.filter((f): f is string => typeof f === "string") : [];
}

/** The project holds a git repository or agent files at its root. */
export function callsForAgents(context: Context): boolean {
  return (
    context.workspace === "project" &&
    (context.git.status === "repo" || agentFilesOf(context).length > 0)
  );
}

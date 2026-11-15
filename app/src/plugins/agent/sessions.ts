// The connected agents as rows: a label that tells same-named agents apart (by start order) and each one's latest
// request (tool from the latest record of that session; the records keep only the newest few, so an older call
// keeps its time without a tool).

import type { AgentRequest, AgentSession } from "../../api";

export type SessionRow = {
  session: AgentSession;
  /** The self-declared name; the second and later agent of one name get " · 2", " · 3"… */
  label: string;
  /** The latest request: its tool when still among the records. */
  last: { tool: string | null; atMs: number } | null;
};

/** `sessions` earliest first (as the backend sends them); `recent` newest first. */
export function sessionRows(
  sessions: readonly AgentSession[],
  recent: readonly AgentRequest[],
): SessionRow[] {
  const seen = new Map<string, number>();
  return sessions.map((session) => {
    const n = (seen.get(session.name) ?? 0) + 1;
    seen.set(session.name, n);
    const record = recent.find((r) => r.session === session.id);
    const last =
      record !== undefined
        ? { tool: record.tool, atMs: record.atMs }
        : session.lastCallMs !== null
          ? { tool: null, atMs: session.lastCallMs }
          : null;
    return { session, label: n === 1 ? session.name : `${session.name} · ${String(n)}`, last };
  });
}

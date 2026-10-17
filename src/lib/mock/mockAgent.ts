// The agent connection for the mock backend: a status kept in memory that changes the way the real one does, an agent
// the mock controls can make come and go and ask things, and the latest context the window published.

import { emitMock } from "../api";
import type { AgentContext, AgentOpen, AgentStatus } from "../types";
import { docExists } from "./fixtures";

const RECENT = 8;

/** Where the mock "installed" PerMark: a path with spaces, so the setup lines show their quoting. */
const PROGRAM = "C:\\Program Files\\PerMark\\permark.exe";

function emptyStatus(): AgentStatus {
  return {
    enabled: false,
    listening: false,
    connections: 0,
    recent: [],
    error: null,
    program: PROGRAM,
  };
}

let status: AgentStatus = emptyStatus();
let latest: AgentContext | null = null;
let waiting: AgentOpen | null = null;

function announce(): void {
  const snapshot = { ...status, recent: [...status.recent] };
  setTimeout(() => emitMock("agent-status", snapshot), 0);
}

export function agentStatus(): AgentStatus {
  return { ...status, recent: [...status.recent] };
}

export function setAgentConnection(enabled: boolean): AgentStatus {
  status = enabled ? { ...status, enabled: true, listening: true, error: null } : emptyStatus();
  announce();
  return agentStatus();
}

/** What the window last published (the mock controls and tests read it). */
export function publishedContext(): AgentContext | null {
  return latest;
}

export function publishAgentContext(context: AgentContext): void {
  latest = context;
}

export function takeAgentOpen(): AgentOpen | null {
  const place = waiting;
  waiting = null;
  return place;
}

/** An agent connects or leaves (the mock controls). Nothing happens while the connection is off. */
export function agentConnects(connected: boolean): void {
  if (!status.listening) return;
  status = { ...status, connections: connected ? 1 : 0 };
  announce();
}

/** An agent asks something (the mock controls). */
export function agentAsks(tool: string, subject: string | null = null): void {
  if (!status.listening) return;
  const recent = [{ tool, subject, atMs: Date.now() }, ...status.recent].slice(0, RECENT);
  status = { ...status, connections: Math.max(1, status.connections), recent };
  announce();
}

/** An agent asks to show a place (the mock controls); the window picks it up as with the real backend. */
export function agentOpens(path: string, line?: number): boolean {
  if (!status.listening || !docExists(path)) return false;
  waiting = line === undefined ? { path } : { path, line };
  agentAsks("open", path);
  setTimeout(() => emitMock("agent-open", null), 0);
  return true;
}

// What the agent plugin's backend half answers in the mock backend (`dev:mock`, tests): the switch, the groups, the
// published context and a status that a probe can fill (a connected client, requests). In memory.

import type { HandOffSessionView } from "./generated/HandOffSessionView";
import type {
  AgentRequest,
  AgentStatus,
  HandOffPreview,
  AgentWindowContext,
  PluginEventName,
  PluginEvents,
  PluginMethod,
  PluginMethods,
} from "./types";

type Handlers = Pick<
  { [M in PluginMethod]: (args: PluginMethods[M]["args"]) => PluginMethods[M]["result"] },
  | "agent.status"
  | "agent.setEnabled"
  | "agent.setGroup"
  | "agent.setDefaults"
  | "agent.publish"
  | "agent.setup"
  | "agent.handOffPreview"
  | "agent.handOffConfirm"
>;

export type AgentProbe = {
  /** What the window published, oldest first. */
  published: AgentWindowContext[];
  /** A client connects (or `null`: it leaves). */
  connect(client: string | null): void;
  /** An agent asked something. */
  request(record: AgentRequest): void;
  /** The switch turned from outside the window (another window, the palette of another view). */
  setEnabled(on: boolean): void;
  /** The server fails to start (`inUse`: another PerMark serves the project; any other text: the reason) | runs. */
  fail(reason: string | null): void;
  status(): AgentStatus;
  /** The hand-offs confirmed, as `{ upto, checked }`. */
  handedOff: { upto: number; checked: number[] }[];
};

const HOUR = 3_600_000;
const DAY = 24 * HOUR;

/** Two sessions of work (today, and three weeks ago: stale) and some changes made outside. */
function sessionsNow(now: number, checked: number[] | undefined): HandOffSessionView[] {
  const base: HandOffSessionView[] = [
    {
      id: 1,
      start: now - 21 * DAY - 3 * HOUR,
      end: now - 21 * DAY - 2 * HOUR,
      actions: 6,
      rows: 4,
      tokens: 310,
      stale: true,
      checked: false,
      dropped: false,
    },
    {
      id: 2,
      start: now - 4 * HOUR,
      end: now - 2 * HOUR,
      actions: 19,
      rows: 12,
      tokens: 1240,
      stale: false,
      checked: true,
      dropped: false,
    },
  ];
  return base.map((s) => ({
    ...s,
    checked: checked === undefined ? s.checked : checked.includes(s.id),
  }));
}

const DEFAULT_OFF = new Set(["comments.resolve"]);
const GROUPS = [
  "context",
  "changes",
  "find",
  "comments.read",
  "comments.resolve",
  "notes",
  "show",
  "activity",
];

export function createMockAgent(
  emit: <E extends PluginEventName>(event: E, data: PluginEvents[E]) => void,
): { handlers: Handlers; probe: AgentProbe } {
  let enabled = false;
  const groups: Record<string, boolean> = Object.fromEntries(
    GROUPS.map((g) => [g, !DEFAULT_OFF.has(g)]),
  );
  let clients: string[] = [];
  let failure: string | null = null;
  let recent: AgentRequest[] = [];
  const published: AgentWindowContext[] = [];
  const handedOff: { upto: number; checked: number[] }[] = [];
  const status = (): AgentStatus => ({
    enabled,
    server: !enabled
      ? { state: "off" }
      : failure !== null
        ? { state: "error", reason: failure }
        : { state: "listening" },
    clients: enabled ? clients : [],
    groups: { ...groups },
    denied: [],
    recent,
  });
  const announce = (): void => emit("agent:status", status());
  const handlers: Handlers = {
    "agent.status": () => status(),
    "agent.setEnabled": ({ enabled: on }) => {
      enabled = on;
      if (!on) clients = [];
      announce();
      return status();
    },
    "agent.setGroup": ({ group, on }) => {
      if (GROUPS.includes(group)) groups[group] = on;
      announce();
      return status();
    },
    "agent.setDefaults": () => null,
    "agent.publish": ({ context }) => {
      published.push(context);
      return null;
    },
    "agent.setup": () => "C:\\Program Files\\PerMark\\PerMark.exe",
    "agent.handOffPreview": ({ checked }): HandOffPreview => {
      if (handedOff.length > 0) return { state: "nothing" };
      const sessions = sessionsNow(Date.now(), checked);
      const inside = sessions.filter((s) => s.checked);
      return {
        state: "ready",
        case: "stale",
        first: false,
        sessions,
        outside: { files: 14, byFolder: false, git: "HEAD a1b2c3d..e4f5a6b (3 commits)" },
        allStale: false,
        trimmed: false,
        upgraded: false,
        rows: inside.reduce((n, s) => n + s.rows, 0) + 14,
        tokens: inside.reduce((n, s) => n + s.tokens, 0) + 420,
        upto: 42,
      };
    },
    "agent.handOffConfirm": ({ upto, checked }) => {
      handedOff.push({ upto, checked });
      const rows = sessionsNow(Date.now(), checked)
        .filter((s) => s.checked)
        .reduce((n, s) => n + s.rows, 14);
      return {
        line: `PerMark: ${String(rows)} actions since last hand-off -> call activity since=handoff`,
        full: `activity since=handoff n=${String(rows)}\n`,
        rows,
      };
    },
  };
  return {
    handlers,
    probe: {
      published,
      setEnabled: (on) => {
        enabled = on;
        if (!on) clients = [];
        announce();
      },
      fail: (reason) => {
        failure = reason;
        announce();
      },
      connect: (client) => {
        clients = client === null ? [] : [client];
        announce();
      },
      request: (record) => {
        recent = [record, ...recent].slice(0, 8);
        announce();
      },
      status,
      handedOff,
    },
  };
}

// What the agent plugin's backend half answers in the mock backend (`dev:mock`, tests): the switch, the groups, the
// published context and a status that a probe can fill (a connected client, requests). In memory.

import type {
  AgentRequest,
  AgentStatus,
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
>;

export type AgentProbe = {
  /** What the window published, oldest first. */
  published: AgentWindowContext[];
  /** A client connects (or `null`: it leaves). */
  connect(client: string | null): void;
  /** An agent asked something. */
  request(record: AgentRequest): void;
  status(): AgentStatus;
};

const DEFAULT_OFF = new Set(["comments.resolve"]);
const GROUPS = ["context", "changes", "find", "comments.read", "comments.resolve", "notes", "show"];

export function createMockAgent(
  emit: <E extends PluginEventName>(event: E, data: PluginEvents[E]) => void,
): { handlers: Handlers; probe: AgentProbe } {
  let enabled = false;
  const groups: Record<string, boolean> = Object.fromEntries(
    GROUPS.map((g) => [g, !DEFAULT_OFF.has(g)]),
  );
  let clients: string[] = [];
  let recent: AgentRequest[] = [];
  const published: AgentWindowContext[] = [];
  const status = (): AgentStatus => ({
    enabled,
    server: enabled ? { state: "listening" } : { state: "off" },
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
  };
  return {
    handlers,
    probe: {
      published,
      connect: (client) => {
        clients = client === null ? [] : [client];
        announce();
      },
      request: (record) => {
        recent = [record, ...recent].slice(0, 8);
        announce();
      },
      status,
    },
  };
}

// Plugin `agent`: the agent connection of a project. Dormant until the person turns it on: then a status item (who is
// connected, what was asked), the context the window publishes, and the places an agent asks to show. The settings
// rows and the hand-off (palette; the status popover too while the switch is on) are there whenever the plugin is on.

import type { Requirement } from "../../core/conditions/available";
import { definePlugin } from "../../core/plugins/registry.svelte";
import { handOff } from "./handoff.svelte";
import { claudeCodeCommand } from "./setup";
import { agent } from "./store.svelte";

const PROJECT: Requirement = { needs: [{ need: "workspace", kinds: ["project"] }], prefers: [] };
const ON: Requirement = {
  needs: [{ need: "workspace", kinds: ["project"] }, { need: "agentOn" }],
  prefers: [],
};

export default definePlugin({
  manifest: { id: "agent", kind: "feature", dependsOn: [] },
  activate(cx) {
    const stop = agent.start();
    const removals = [
      cx.contribute({
        kind: "statusItem",
        id: "agent.status",
        slot: "right",
        order: 3,
        requires: ON,
        view: () => import("./AgentStatus.svelte"),
      }),
      // draws nothing in the bar: the hand-off dialog over the window while it is open
      cx.contribute({
        kind: "statusItem",
        id: "agent.handOffHost",
        slot: "right",
        order: 96,
        requires: PROJECT,
        view: () => import("./HandOffHost.svelte"),
      }),
      cx.contribute({
        kind: "command",
        id: "agent.handOff",
        label: "agent.handOff",
        requires: PROJECT,
        run: () => void handOff.open(),
      }),
      cx.contribute({
        kind: "settingsSection",
        id: "agent.settings",
        label: "agent.title",
        requires: PROJECT,
        view: () => import("./AgentSettings.svelte"),
      }),
      cx.contribute({
        kind: "projectSection",
        id: "agent.narrowing",
        label: "agent.narrow.title",
        requires: PROJECT,
        view: () => import("./AgentNarrowing.svelte"),
      }),
      cx.contribute({
        kind: "command",
        id: "agent.turnOn",
        label: "agent.turnOn",
        requires: PROJECT,
        when: () => !agent.enabled,
        run: () => void agent.setEnabled(true),
      }),
      cx.contribute({
        kind: "command",
        id: "agent.turnOff",
        label: "agent.turnOff",
        requires: ON,
        when: () => agent.enabled,
        run: () => void agent.setEnabled(false),
      }),
      cx.contribute({
        kind: "command",
        id: "agent.copySetup",
        label: "agent.copySetupCommand",
        requires: PROJECT,
        run: async () => {
          if (agent.program === null) await agent.loadProgram();
          await navigator.clipboard.writeText(claudeCodeCommand(agent.program));
        },
      }),
    ];
    return () => {
      for (const remove of removals) remove();
      agent.popover = false;
      handOff.close();
      stop();
    };
  },
});

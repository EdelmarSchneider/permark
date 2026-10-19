<script lang="ts">
  // Settings rows of the agent connection (project windows): the switch for this project, what agents may do, and
  // the lines that connect an agent. Present while the plugin is on, even with the switch off.
  import { t, type MessageKey } from "../../core/i18n";
  import { GROUPS } from "./groups";
  import { claudeCodeCommand, hookJson, mcpJson } from "./setup";
  import { agent } from "./store.svelte";

  const status = $derived(agent.status);
  let open = $state(false);
  let copied = $state<string | null>(null);

  const SETUP: { id: string; label: MessageKey; text: (program: string | null) => string }[] = [
    { id: "claude", label: "agent.setup.claude", text: claudeCodeCommand },
    { id: "json", label: "agent.setup.json", text: mcpJson },
    { id: "hook", label: "agent.setup.hook", text: hookJson },
  ];

  async function toggleSetup(): Promise<void> {
    open = !open;
    copied = null;
    if (open && agent.program === null) await agent.loadProgram();
  }

  async function copy(id: string, text: string): Promise<void> {
    await navigator.clipboard.writeText(text);
    copied = id;
  }
</script>

{#if status !== null}
  <div class="agent-settings">
    <label class="row" title={t("agent.switchHint")}>
      <span>{t("agent.switch")}</span>
      <input
        type="checkbox"
        role="switch"
        checked={status.enabled}
        onchange={(e) => void agent.setEnabled(e.currentTarget.checked)}
      />
    </label>
    {#if status.enabled}
      <fieldset class="groups">
        <legend>{t("agent.groups")}</legend>
        {#each GROUPS as group (group.id)}
          {@const denied = status.denied.includes(group.id)}
          <label class="group" title={denied ? t("agent.denied") : ""}>
            <input
              type="checkbox"
              checked={status.groups[group.id] === true && !denied}
              disabled={denied}
              onchange={(e) => void agent.setGroup(group.id, e.currentTarget.checked)}
            />
            <span class:denied>{t(group.label)}</span>
          </label>
        {/each}
        <button type="button" class="link" onclick={() => void agent.setDefaults()}>
          {t("agent.defaults")}
        </button>
      </fieldset>
    {/if}
    <div class="row">
      <span>{t("agent.setup")}</span>
      <button
        type="button"
        class="btn btn--sm"
        aria-expanded={open}
        onclick={() => void toggleSetup()}
      >
        {open ? t("common.close") : t("agent.setup.show")}
      </button>
    </div>
    {#if open}
      <div class="setup">
        {#each SETUP as line (line.id)}
          {@const text = line.text(agent.program)}
          <div class="setup-line">
            <div class="setup-head">
              <span>{t(line.label)}</span>
              <button type="button" class="btn btn--sm" onclick={() => void copy(line.id, text)}>
                {copied === line.id ? t("agent.copied") : t("agent.copy")}
              </button>
            </div>
            <pre>{text}</pre>
          </div>
        {/each}
      </div>
    {/if}
  </div>
{/if}

<style>
  .agent-settings {
    border-top: 1px solid var(--border);
    margin-top: 4px;
    padding-top: 4px;
  }
  .row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--sp-3);
    padding: 6px 10px;
  }
  .groups {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 4px var(--sp-3);
    margin: 0 10px 6px;
    padding: 6px 8px;
    border: 1px solid var(--border);
    border-radius: var(--radius-control);
  }
  legend {
    padding: 0 4px;
    color: var(--muted);
    font-size: var(--fs-ui-xs);
  }
  .group {
    display: flex;
    align-items: center;
    gap: 6px;
    font-size: var(--fs-ui-sm);
  }
  .denied {
    color: var(--muted);
    text-decoration: line-through;
  }
  .link {
    grid-column: 1 / -1;
    justify-self: start;
    padding: 0;
    border: 0;
    background: none;
    color: var(--accent);
    font: inherit;
    font-size: var(--fs-ui-sm);
    cursor: pointer;
  }
  .setup {
    display: grid;
    gap: var(--sp-2);
    padding: 0 10px 8px;
  }
  .setup-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--sp-2);
    font-size: var(--fs-ui-sm);
  }
  pre {
    margin: 4px 0 0;
    padding: 6px 8px;
    overflow-x: auto;
    border-radius: var(--radius-control);
    background: var(--surface);
    font-family: var(--font-code);
    font-size: var(--fs-ui-xs);
    white-space: pre;
  }
</style>

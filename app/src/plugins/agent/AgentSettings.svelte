<script lang="ts">
  // Settings rows of the agent connection (project windows): the switch for this project, what agents may do, and
  // the lines that connect an agent. Present while the plugin is on, even with the switch off.
  import { t, type MessageKey } from "../../core/i18n";
  import SettingRow from "../../core/shell/settings/SettingRow.svelte";
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
  <SettingRow label={t("agent.switch")} labelFor="set-agent" hint={t("agent.switchHint")}>
    <input
      id="set-agent"
      type="checkbox"
      role="switch"
      checked={status.enabled}
      onchange={(e) => void agent.setEnabled(e.currentTarget.checked)}
    />
  </SettingRow>
  {#if status.enabled}
    <SettingRow label={t("agent.groups")} wide>
      <fieldset class="groups">
        <legend class="sr-only">{t("agent.groups")}</legend>
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
      </fieldset>
      {#snippet note()}
        <button type="button" class="link" onclick={() => void agent.setDefaults()}>
          {t("agent.defaults")}
        </button>
      {/snippet}
    </SettingRow>
  {/if}
  <SettingRow label={t("agent.setup")}>
    <button
      type="button"
      class="btn btn--sm"
      aria-expanded={open}
      onclick={() => void toggleSetup()}
    >
      {open ? t("common.close") : t("agent.setup.show")}
    </button>
  </SettingRow>
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
{/if}

<style>
  .groups {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(200px, 1fr));
    gap: var(--sp-1) var(--sp-4);
    min-width: 0;
    margin: 0;
    padding: 0;
    border: 0;
  }
  .group {
    display: flex;
    align-items: center;
    gap: var(--sp-2);
    font-size: var(--fs-ui-sm);
  }
  .denied {
    color: var(--muted);
    text-decoration: line-through;
  }
  .link {
    justify-self: start;
    padding: 0;
    border: 0;
    background: none;
    color: var(--accent-text);
    font: inherit;
    font-size: var(--fs-ui-sm);
    text-decoration: underline;
    cursor: pointer;
  }
  .setup {
    display: grid;
    gap: var(--sp-3);
    padding: var(--sp-3) var(--sp-4) var(--sp-4);
  }
  .setup-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--sp-2);
    font-size: var(--fs-ui-sm);
  }
  pre {
    margin: var(--sp-1) 0 0;
    padding: var(--sp-2);
    overflow-x: auto;
    border-radius: var(--radius-control);
    background: var(--surface);
    font-family: var(--font-code);
    font-size: var(--fs-ui-xs);
    white-space: pre;
  }
</style>

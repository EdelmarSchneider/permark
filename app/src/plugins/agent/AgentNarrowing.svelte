<script lang="ts">
  // What the project itself never allows agents: groups and paths in `.permark/config.toml`. A project can only narrow
  // what each person allows (the switch and the groups of the agent connection stay the person's own).
  import ListEditor from "../../core/shell/settings/ListEditor.svelte";
  import { problemKey } from "../../core/shell/settings/problems";
  import SettingRow from "../../core/shell/settings/SettingRow.svelte";
  import { projectSettings } from "../../core/shell/settings/stores.svelte";
  import { t } from "../../core/i18n";
  import { GROUPS } from "./groups";

  const settings = $derived(
    projectSettings.state.status === "ready" ? projectSettings.state.settings : null,
  );
  const problem = $derived(projectSettings.problem);

  function toggle(group: string, deny: boolean): void {
    if (settings === null) return;
    const rest = settings.agentDeny.filter((g) => g !== group);
    void projectSettings.set({ key: "agentDeny", value: deny ? [...rest, group] : rest });
  }
</script>

{#if settings !== null}
  <SettingRow label={t("agent.narrow.groups")} hint={t("agent.narrow.groupsHint")} wide>
    <fieldset class="groups">
      <legend class="sr-only">{t("agent.narrow.groups")}</legend>
      {#each GROUPS as group (group.id)}
        <label class="group">
          <input
            type="checkbox"
            checked={settings.agentDeny.includes(group.id)}
            onchange={(e) => toggle(group.id, e.currentTarget.checked)}
          />
          <span>{t(group.label)}</span>
        </label>
      {/each}
    </fieldset>
    {#snippet note()}
      {#if problem !== null && problem.key === "agentDeny"}
        <span class="problem" role="alert">{t(problemKey(problem.code))}</span>
      {:else}
        {t("agent.narrow.groupsNote")}
      {/if}
    {/snippet}
  </SettingRow>
  <SettingRow label={t("agent.narrow.paths")} hint={t("agent.narrow.pathsHint")} wide>
    <ListEditor
      items={settings.agentDenyPaths}
      label={t("agent.narrow.paths")}
      placeholder={t("project.patternPlaceholder")}
      onchange={(items) => void projectSettings.set({ key: "agentDenyPaths", value: items })}
    />
    {#snippet note()}
      {#if problem !== null && problem.key === "agentDenyPaths"}
        <span class="problem" role="alert">{t(problemKey(problem.code))}</span>
      {/if}
    {/snippet}
  </SettingRow>
{/if}

<style>
  .groups {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(190px, 1fr));
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
  .problem {
    color: var(--git-deleted-text);
  }
</style>

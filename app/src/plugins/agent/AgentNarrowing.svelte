<script lang="ts">
  // What the folder itself never allows agents, for everyone who opens it: groups and paths in `.permark/config.toml`.
  // A folder can only narrow what each person allows (the switch and the groups above stay the person's own). Without
  // `.permark/` the rows show greyed beside the offer to set PerMark up: nothing is written before.
  import { conditions } from "../../core/conditions/context.svelte";
  import Card from "../../core/shell/settings/Card.svelte";
  import ListEditor from "../../core/shell/settings/ListEditor.svelte";
  import { problemKey } from "../../core/shell/settings/problems";
  import SettingRow from "../../core/shell/settings/SettingRow.svelte";
  import ShareInit from "../../core/shell/settings/ShareInit.svelte";
  import { projectSettings } from "../../core/shell/settings/stores.svelte";
  import { t } from "../../core/i18n";
  import { GROUPS } from "./groups";

  const settings = $derived(
    projectSettings.state.status === "ready" ? projectSettings.state.settings : null,
  );
  const off = $derived(conditions.current.permark !== "initialized");
  const problem = $derived(projectSettings.problem);

  function toggle(group: string, deny: boolean): void {
    if (settings === null) return;
    const rest = settings.agentDeny.filter((g) => g !== group);
    void projectSettings.set({ key: "agentDeny", value: deny ? [...rest, group] : rest });
  }
</script>

{#snippet problemNote()}
  {#if problem !== null}
    <span class="problem" role="alert">{t(problemKey(problem.code))}</span>
  {/if}
{/snippet}

{#if settings !== null}
  {#if off}
    <ShareInit compact />
  {/if}
  <div class="limits-rows" class:off>
    <Card>
      <SettingRow
        label={t("agent.narrow.groups")}
        hint={t("agent.narrow.groupsHint")}
        scope="everyone"
        wide
        note={problem?.key === "agentDeny" ? problemNote : undefined}
      >
        <fieldset class="groups">
          <legend class="sr-only">{t("agent.narrow.groups")}</legend>
          {#each GROUPS as group (group.id)}
            <label class="group">
              <input
                type="checkbox"
                checked={settings.agentDeny.includes(group.id)}
                disabled={off}
                onchange={(e) => toggle(group.id, e.currentTarget.checked)}
              />
              <span>{t(group.label)}</span>
            </label>
          {/each}
        </fieldset>
      </SettingRow>
      <SettingRow
        label={t("agent.narrow.paths")}
        hint={t("agent.narrow.pathsHint")}
        scope="everyone"
        wide
        note={problem?.key === "agentDenyPaths" ? problemNote : undefined}
      >
        <ListEditor
          items={settings.agentDenyPaths}
          label={t("agent.narrow.paths")}
          placeholder={t("agent.narrow.pathsPlaceholder")}
          disabled={off}
          onchange={(items) => void projectSettings.set({ key: "agentDenyPaths", value: items })}
        />
      </SettingRow>
    </Card>
  </div>
{/if}

<style>
  /* without .permark/ the shared rows only show what they would hold */
  .limits-rows.off {
    opacity: 0.6;
  }
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

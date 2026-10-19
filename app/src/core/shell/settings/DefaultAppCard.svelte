<script lang="ts">
  // Settings -> Integration -> Default app: one row per group of file types. A check where PerMark can set it (macOS,
  // an installed Linux package); who opens the files now + "Choose in Windows Settings" on Windows; one sentence when
  // this copy cannot (an AppImage, a snap).
  import { onMount } from "svelte";
  import type { FileGroup } from "../../../api";
  import { t, type MessageKey } from "../../i18n";
  import { defaultApp } from "../defaultApp.svelte";
  import SettingRow from "./SettingRow.svelte";

  const NAMES: Readonly<Record<FileGroup, MessageKey>> = {
    markdown: "fileGroup.markdown",
    text: "fileGroup.text",
    json: "fileGroup.json",
    xml: "fileGroup.xml",
    yaml: "fileGroup.yaml",
    toml: "fileGroup.toml",
    ini: "fileGroup.ini",
    properties: "fileGroup.properties",
  };

  onMount(() => {
    void defaultApp.load();
  });

  const view = $derived(defaultApp.view);
</script>

{#if view !== null}
  {#if view.mode.mode === "unsupported"}
    <p class="line">
      {view.mode.reason === "confined" ? t("defaultApp.confined") : t("defaultApp.notInstalled")}
    </p>
  {:else}
    {#if view.mode.mode === "handoff"}
      <SettingRow label={t("defaultApp.handoffRow")} hint={t("defaultApp.handoffHint")}>
        <button type="button" class="btn" onclick={() => void defaultApp.handoff()}>
          {t("defaultApp.handoff")}
        </button>
      </SettingRow>
    {/if}
    {#each view.groups as group (group.group)}
      <SettingRow
        label={t(NAMES[group.group])}
        labelFor={view.mode.mode === "direct" ? `set-default-${group.group}` : undefined}
        labelId={view.mode.mode === "direct" ? undefined : `default-${group.group}`}
        hint={group.extensions.join("  ")}
      >
        {#if view.mode.mode === "direct"}
          <input
            id={`set-default-${group.group}`}
            type="checkbox"
            role="switch"
            checked={group.ours}
            onchange={(e) => void defaultApp.set(group.group, e.currentTarget.checked)}
          />
        {:else}
          <span class="state" class:ours={group.ours} aria-labelledby={`default-${group.group}`}>
            {group.ours ? t("defaultApp.ours") : t("defaultApp.other")}
          </span>
        {/if}
      </SettingRow>
    {/each}
  {/if}
  {#if defaultApp.problem !== null}
    <p class="line problem" role="alert">{defaultApp.problem}</p>
  {/if}
{/if}

<style>
  .line {
    margin: 0;
    padding: var(--sp-3) var(--sp-4);
    color: var(--muted);
    font-size: var(--fs-ui-sm);
  }
  .problem {
    color: var(--git-deleted-text);
  }
  .state {
    color: var(--muted);
    font-size: var(--fs-ui-sm);
    white-space: nowrap;
  }
  .state.ours {
    color: var(--git-added-text);
  }
</style>

<script lang="ts">
  // This folder > Plugins here: the plugins that work per folder, each with its choice for this folder - the app's
  // switch, on or off for this person only (kept on this computer), or off for everyone (kept in the folder's
  // `.permark/config.toml`, which wins over each person's choice). Off for everyone waits for Initialize while
  // `.permark/` is missing. The row's chip says who the choice in force is for.
  import type { PluginInfo } from "../../../api";
  import { api } from "../../../api";
  import { conditions } from "../../conditions/context.svelte";
  import { t, type MessageKey } from "../../i18n";
  import type { WindowStore } from "../../window.svelte";
  import Card from "./Card.svelte";
  import {
    aboutKey,
    choiceOf,
    choiceScope,
    choiceWrite,
    nameKey,
    type ProjectChoice,
  } from "./pluginsModel";
  import { problemKey } from "./problems";
  import SettingRow from "./SettingRow.svelte";
  import ShareInit from "./ShareInit.svelte";
  import { pluginsStore, projectSettings } from "./stores.svelte";

  type Props = { store: WindowStore };
  let { store }: Props = $props();

  // the list is read when the section shows and follows the plugins and this view's conditions (external I/O)
  $effect(() => pluginsStore.follow(store.label));

  const initialized = $derived(conditions.current.permark === "initialized");
  const listing = $derived(pluginsStore.state);
  const infos = $derived(
    listing.status === "ready"
      ? listing.plugins.filter((info) => info.manifest.scope === "project")
      : [],
  );
  const problem = $derived(projectSettings.problem);
  /** The plugin whose Off for everyone was refused (its row says why, once). */
  let refused = $state<string | null>(null);

  const CHOICES: { value: ProjectChoice; label: MessageKey }[] = [
    { value: "app", label: "plugins.choice.app" },
    { value: "on", label: "plugins.choice.on" },
    { value: "off", label: "plugins.choice.off" },
    { value: "offEveryone", label: "plugins.choice.offEveryone" },
  ];

  function nameOf(id: string): string {
    const key = nameKey(id);
    return key === null ? id : t(key);
  }

  function aboutOf(id: string): string {
    const key = aboutKey(id);
    return key === null ? "" : t(key);
  }

  /** Writes what the new choice changes: the folder's switch first (it wins), then the person's own. */
  async function choose(info: PluginInfo, next: ProjectChoice): Promise<void> {
    const write = choiceWrite(info, next);
    const plugin = info.manifest.id;
    const kept =
      write.teamOff === null ||
      (await projectSettings.set({ key: "pluginOff", value: { plugin, off: write.teamOff } }));
    refused = kept ? null : plugin;
    if (kept && write.person !== undefined) {
      const result = await api.pluginSetProject(plugin, write.person);
      if (!result.ok) store.notifyFileOpFailed(result.error);
    }
    await pluginsStore.load();
  }
</script>

{#if listing.status === "failed"}
  <p class="quiet" role="alert">{t("project.loadFailed")}</p>
{:else if listing.status === "loading"}
  <p class="quiet">{t("app.loading")}</p>
{:else}
  <p class="intro">{t("plugins.hereIntro")}</p>
  {#if !initialized}
    <ShareInit />
  {/if}
  <Card>
    {#each infos as info (info.manifest.id)}
      {@const id = info.manifest.id}
      {@const scope = choiceScope(info)}
      {@const failed = refused === id && problem !== null && problem.key === "pluginOff"}
      {#snippet rowNote()}
        {#if failed && problem !== null}
          <span class="problem" role="alert">{t(problemKey(problem.code))}</span>
        {:else if scope === "everyone"}
          {t("plugins.offEveryone")}
        {:else}
          {t("plugins.offHere")}
        {/if}
      {/snippet}
      <SettingRow
        label={nameOf(id)}
        labelFor={`here-${id}`}
        hint={aboutOf(id)}
        scope={scope ?? undefined}
        note={failed || scope === "everyone" || info.projectChoice === false ? rowNote : undefined}
      >
        <select
          id={`here-${id}`}
          class="field-select"
          value={choiceOf(info)}
          onchange={(e) => {
            const found = CHOICES.find((c) => c.value === e.currentTarget.value);
            if (found) void choose(info, found.value);
          }}
        >
          {#each CHOICES as choice (choice.value)}
            <option value={choice.value} disabled={choice.value === "offEveryone" && !initialized}
              >{t(choice.label)}</option
            >
          {/each}
        </select>
      </SettingRow>
    {/each}
  </Card>
{/if}

<style>
  .intro,
  .quiet {
    margin: 0 0 var(--sp-4);
    color: var(--muted);
    font-size: var(--fs-ui-sm);
  }
  .problem {
    color: var(--git-deleted-text);
  }
</style>

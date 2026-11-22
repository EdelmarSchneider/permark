<script lang="ts">
  // Plugins: every plugin of this build grouped by kind, each with its switch. Off leaves nothing behind (its data can
  // be removed); a plugin that is on but cannot work in this window says why in one sentence, with the fix when there
  // is one. Only the app-wide switch here: a folder's own plugin choices belong with that folder's settings.
  import { tick } from "svelte";
  import type { PluginInfo } from "../../../api";
  import { remedyAction } from "../../commands/remedy";
  import { conditions } from "../../conditions/context.svelte";
  import { reasonKey, remedyKey } from "../../conditions/guidance";
  import { sizeText, t } from "../../i18n";
  import { api } from "../../../api";
  import { apiErrorText } from "../../errors";
  import type { WindowStore } from "../../window.svelte";
  import Card from "./Card.svelte";
  import { aboutKey, canRemoveData, grouped, kindKey, nameKey, statusOf } from "./pluginsModel";
  import { pluginsStore } from "./stores.svelte";

  type Props = { store: WindowStore };
  let { store }: Props = $props();

  // the list is read when the section shows and follows the plugins and this view's conditions (external I/O)
  $effect(() => pluginsStore.follow(store.label));

  const listing = $derived(pluginsStore.state);
  const infos = $derived(listing.status === "ready" ? listing.plugins : []);
  const groups = $derived(grouped(infos));
  /** The plugin whose "Remove data" is being confirmed, with the bytes it frees (`null`: not known). */
  let confirming = $state<{ id: string; bytes: number | null } | null>(null);

  function nameOf(id: string): string {
    const key = nameKey(id);
    return key === null ? id : t(key);
  }

  function aboutOf(id: string): string {
    const key = aboutKey(id);
    return key === null ? "" : t(key);
  }

  async function setApp(info: PluginInfo, on: boolean): Promise<void> {
    await store.changeSetting({
      key: "pluginEnabled",
      value: { plugin: info.manifest.id, enabled: on },
    });
    await pluginsStore.load();
  }

  // Remove data and its question take each other's place in a row: the keyboard on the one that went goes to the one
  // drawn now (the question: its Cancel, the safe answer), never to the page body
  /** The question's Cancel (one question at a time). */
  let cancelButton: HTMLButtonElement | undefined = $state();
  /** Each row's Remove data button, by plugin. */
  const removeButtons: Record<string, HTMLButtonElement | undefined> = $state({});

  /** Asks the question; the bytes the removal frees join it once the backend tells them. */
  async function askRemove(id: string): Promise<void> {
    confirming = { id, bytes: null };
    void tick().then(() => cancelButton?.focus());
    const result = await api.pluginDataBytes(id);
    if (confirming?.id === id && result.ok) confirming = { id, bytes: result.value };
  }

  /** The question goes, the keyboard back on the row's Remove data. */
  function closeQuestion(id: string): void {
    confirming = null;
    void tick().then(() => removeButtons[id]?.focus());
  }

  function removeQuestion(id: string, bytes: number | null): string {
    const name = nameOf(id);
    return bytes !== null && bytes > 0
      ? t("plugins.removeConfirmBytes", { name, size: sizeText(bytes) })
      : t("plugins.removeConfirm", { name });
  }

  async function removeData(info: PluginInfo): Promise<void> {
    closeQuestion(info.manifest.id);
    const result = await api.pluginRemoveData(info.manifest.id);
    if (result.ok) store.notifyNote(t("plugins.dataRemoved", { name: nameOf(info.manifest.id) }));
    else store.notifyNote(t("plugins.dataNotRemoved", { message: apiErrorText(result.error) }));
  }

  async function turnOn(id: string): Promise<void> {
    await store.changeSetting({ key: "pluginEnabled", value: { plugin: id, enabled: true } });
    await pluginsStore.load();
  }
</script>

{#if listing.status === "loading"}
  <p class="quiet">{t("app.loading")}</p>
{:else if listing.status === "failed"}
  <p class="quiet" role="alert">{apiErrorText(listing.error)}</p>
{:else}
  <p class="intro">{t("plugins.intro")}</p>
  {#each groups as group (group.kind)}
    <section class="group" aria-labelledby={`plugins-${group.kind}`}>
      <h4 id={`plugins-${group.kind}`}>{t(kindKey(group.kind))}</h4>
      <Card>
        {#each group.plugins as info (info.manifest.id)}
          {@const id = info.manifest.id}
          {@const status = statusOf(info, conditions.current)}
          <div class="plugin" class:is-off={!info.appOn}>
            <div class="text">
              <span class="name" id={`plugin-${id}`}>{nameOf(id)}</span>
              <span class="about">{aboutOf(id)}</span>
              {#if status.kind === "needs"}
                {@const run = remedyAction(status.reason.remedy)}
                {@const remedyLabel = remedyKey(status.reason.remedy)}
                <span class="status">
                  {t(reasonKey(status.reason))}
                  {#if run && remedyLabel}
                    <button type="button" class="link" onclick={run}>{t(remedyLabel)}</button>
                  {/if}
                </span>
              {:else if status.kind === "dependency"}
                <span class="status">
                  {t("plugins.needs", { name: nameOf(status.plugin) })}
                  <button type="button" class="link" onclick={() => void turnOn(status.plugin)}>
                    {t("plugins.turnOn", { name: nameOf(status.plugin) })}
                  </button>
                </span>
              {:else if status.kind === "project"}
                <span class="status">{t("plugins.offHere")}</span>
              {/if}
            </div>
            <div class="control">
              {#if canRemoveData(info) && confirming?.id !== id}
                <button
                  type="button"
                  class="btn btn--ghost btn--sm"
                  bind:this={removeButtons[id]}
                  onclick={() => void askRemove(id)}
                >
                  {t("plugins.removeData")}
                </button>
              {/if}
              <input
                type="checkbox"
                role="switch"
                checked={info.appOn}
                aria-labelledby={`plugin-${id}`}
                aria-checked={info.appOn}
                onchange={(e) => void setApp(info, e.currentTarget.checked)}
              />
            </div>
            {#if canRemoveData(info) && confirming?.id === id}
              <div class="confirm" role="group" aria-label={t("plugins.removeData")}>
                <!-- a folder keeping its data in its own .permark/local is reached only through its open window -->
                <span>{removeQuestion(id, confirming.bytes)} {t("plugins.removeScope")}</span>
                <button type="button" class="btn btn--sm" onclick={() => void removeData(info)}>
                  {t("plugins.removeYes")}
                </button>
                <button
                  type="button"
                  class="btn btn--ghost btn--sm"
                  bind:this={cancelButton}
                  onclick={() => closeQuestion(id)}
                >
                  {t("common.cancel")}
                </button>
              </div>
            {/if}
          </div>
        {/each}
      </Card>
    </section>
  {/each}
{/if}

<style>
  .intro,
  .quiet {
    margin: 0 0 var(--sp-4);
    color: var(--muted);
    font-size: var(--fs-ui-sm);
  }
  .group + .group {
    margin-top: var(--sp-5);
  }
  h4 {
    margin: 0 0 var(--sp-2);
    padding-left: var(--sp-1);
    color: var(--muted);
    font-size: var(--fs-ui-sm);
    font-weight: 600;
  }
  .plugin {
    display: grid;
    grid-template-columns: minmax(0, 1fr) auto;
    align-items: start;
    column-gap: var(--sp-5);
    padding: var(--sp-3) var(--sp-4);
  }
  .plugin + .plugin {
    border-top: 1px solid var(--border);
  }
  .text {
    display: flex;
    flex-direction: column;
    gap: 2px;
    min-width: 0;
  }
  .name {
    font-weight: 500;
  }
  .plugin.is-off .name {
    color: var(--muted);
  }
  .about {
    color: var(--muted);
    font-size: var(--fs-ui-sm);
    overflow-wrap: anywhere;
  }
  .status {
    color: var(--faint);
    font-size: var(--fs-ui-sm);
    overflow-wrap: anywhere;
  }
  .link {
    margin-left: var(--sp-1);
    padding: 0;
    border: 0;
    background: none;
    color: var(--accent-text);
    font: inherit;
    text-decoration: underline;
    cursor: pointer;
  }
  .control {
    display: flex;
    align-items: center;
    justify-content: flex-end;
    flex-wrap: wrap;
    gap: var(--sp-3);
    min-height: 24px;
  }
  .confirm {
    grid-column: 1 / -1;
    display: flex;
    align-items: center;
    flex-wrap: wrap;
    gap: var(--sp-2);
    margin-top: var(--sp-2);
    padding: var(--sp-2) var(--sp-3);
    border-radius: var(--radius-control);
    background: var(--surface);
    font-size: var(--fs-ui-sm);
  }
  @media (max-width: 640px) {
    .plugin {
      grid-template-columns: minmax(0, 1fr);
      row-gap: var(--sp-2);
    }
    .control {
      justify-content: flex-start;
    }
  }
</style>

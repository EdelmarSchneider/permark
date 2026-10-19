<script lang="ts">
  // Settings rows of the Notes: the folder the notes are in, and whether the app starts on the welcome screen or on
  // the Notes.
  import { t, type MessageKey } from "../../core/i18n";
  import { pluginClient } from "../../core/plugins/client";
  import { windowStore } from "../../core/window.svelte";
  import { api, type StartIn } from "../../api";

  const START: { value: StartIn; label: MessageKey }[] = [
    { value: "welcome", label: "notes.settings.startWelcome" },
    { value: "notes", label: "notes.settings.startNotes" },
  ];

  let place = $state("");
  let isDefault = $state(true);

  // the folder as the backend resolves it (the default included); read again when the setting changes
  $effect(() => {
    void windowStore.notesFolder;
    let gone = false;
    void pluginClient.call("notes.folder", {}).then((result) => {
      if (gone || !result.ok) return;
      place = result.value.path;
      isDefault = result.value.isDefault;
    });
    return () => {
      gone = true;
    };
  });

  async function change(): Promise<void> {
    const picked = await api.pickFolder(t("notes.settings.pickTitle"));
    if (picked !== null) await windowStore.changeSetting({ key: "notesFolder", value: picked });
  }
</script>

<div class="row" title={t("notes.settings.folderHint")}>
  <span>{t("notes.settings.folder")}</span>
  <div class="value">
    <span class="path" title={place}>{place}</span>
    <button type="button" class="btn btn--sm" onclick={() => void change()}>
      {t("notes.settings.change")}
    </button>
    {#if !isDefault}
      <button
        type="button"
        class="btn btn--sm btn--ghost"
        onclick={() => void windowStore.changeSetting({ key: "notesFolder", value: "" })}
      >
        {t("notes.settings.useDefault")}
      </button>
    {/if}
  </div>
</div>
<div class="row">
  <span id="start-in-label">{t("notes.settings.startIn")}</span>
  <div class="segments" role="radiogroup" aria-labelledby="start-in-label">
    {#each START as option (option.value)}
      <button
        type="button"
        role="radio"
        aria-checked={windowStore.startIn === option.value}
        class={["segment", windowStore.startIn === option.value && "is-on"]}
        onclick={() => void windowStore.changeSetting({ key: "startIn", value: option.value })}
        >{t(option.label)}</button
      >
    {/each}
  </div>
</div>

<style>
  .row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--sp-3);
    padding: 6px 10px;
  }
  .value {
    display: flex;
    align-items: center;
    gap: var(--sp-1);
    min-width: 0;
  }
  .path {
    max-width: 150px;
    overflow: hidden;
    color: var(--muted);
    font-size: var(--fs-ui-sm);
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .segments {
    display: inline-flex;
    padding: 2px;
    border: 1px solid var(--border);
    border-radius: var(--radius-control);
    background: var(--surface);
  }
  .segment {
    height: 22px;
    padding: 0 8px;
    border: none;
    border-radius: 4px;
    background: transparent;
    color: var(--muted);
    font-size: var(--fs-ui-sm);
    cursor: pointer;
  }
  .segment.is-on {
    background: var(--bg);
    color: var(--text);
    box-shadow: 0 0 0 1px var(--border);
  }
</style>

<script lang="ts">
  // Settings rows of the Notes: the folder the notes are in, and whether the app starts on the welcome screen or on
  // the Notes.
  import { t, type MessageKey } from "../../core/i18n";
  import { pluginClient } from "../../core/plugins/client";
  import Segments from "../../core/shell/settings/Segments.svelte";
  import SettingRow from "../../core/shell/settings/SettingRow.svelte";
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

<SettingRow label={t("notes.settings.folder")} hint={t("notes.settings.folderHint")} wide>
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
</SettingRow>
<SettingRow label={t("notes.settings.startIn")} labelId="start-in-label">
  <Segments
    labelledby="start-in-label"
    value={windowStore.startIn}
    options={START.map((o) => ({ value: o.value, label: t(o.label) }))}
    onchange={(value) => void windowStore.changeSetting({ key: "startIn", value })}
  />
</SettingRow>

<style>
  .value {
    display: flex;
    align-items: center;
    gap: var(--sp-2);
    min-width: 0;
  }
  .path {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    color: var(--muted);
    font-family: var(--font-code);
    font-size: var(--fs-ui-xs);
    text-overflow: ellipsis;
    white-space: nowrap;
  }
</style>

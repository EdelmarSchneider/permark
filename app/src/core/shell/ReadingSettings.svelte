<script lang="ts">
  // Reading rows of the settings: how "copy reference" is written, and the editor that code references and "open in
  // editor" use. Changes are kept at once; a value the backend refuses is told in a quiet notice.
  import { EDITOR_PRESETS, presetOf } from "../doc/editorPresets";
  import { DEFAULT_REFERENCE_TEMPLATE, formatReference } from "../doc/reference";
  import { t } from "../i18n";
  import type { WindowStore } from "../window.svelte";

  type Props = { store: WindowStore };
  let { store }: Props = $props();

  /** The choice the person made in the list; until then the one the saved command is. */
  let chosen = $state<string | null>(null);
  const mode = $derived(chosen ?? presetOf(store.editor));

  const preview = $derived(
    formatReference(store.referenceTemplate, { path: "docs/plan.md", section: "scope", line: 12 }),
  );

  function pick(id: string): void {
    chosen = id;
    if (id === "custom") return;
    const command = id === "none" ? "" : (EDITOR_PRESETS.find((p) => p.id === id)?.command ?? "");
    void store.changeSetting({ key: "editor", value: command });
  }
</script>

<div class="field">
  <label for="reference-template">{t("settings.refTemplate")}</label>
  <input
    id="reference-template"
    type="text"
    spellcheck="false"
    autocomplete="off"
    value={store.referenceTemplate}
    placeholder={DEFAULT_REFERENCE_TEMPLATE}
    onchange={(e) =>
      void store.changeSetting({ key: "referenceTemplate", value: e.currentTarget.value })}
  />
  <p class="hint">{t("settings.referenceHint")} <code>{preview}</code></p>
</div>

<div class="field">
  <label for="editor-preset">{t("settings.editor")}</label>
  <select id="editor-preset" value={mode} onchange={(e) => pick(e.currentTarget.value)}>
    <option value="none">{t("settings.editor.none")}</option>
    {#each EDITOR_PRESETS as preset (preset.id)}
      <option value={preset.id}>{t(preset.label)}</option>
    {/each}
    <option value="custom">{t("settings.editor.custom")}</option>
  </select>
  {#if mode === "custom"}
    <input
      type="text"
      class="command"
      spellcheck="false"
      autocomplete="off"
      aria-label={t("settings.editorCommand")}
      value={store.editor}
      placeholder={t("settings.editorPlaceholder")}
      onchange={(e) => void store.changeSetting({ key: "editor", value: e.currentTarget.value })}
    />
    <p class="hint">{t("settings.editorHint")}</p>
  {/if}
</div>

<style>
  .field {
    display: flex;
    flex-direction: column;
    gap: 4px;
    padding: 6px 10px;
  }
  input,
  select {
    height: 28px;
    padding: 0 8px;
    border: 1px solid var(--border);
    border-radius: var(--radius-control);
    background: var(--bg);
    color: var(--text);
  }
  .command {
    font-family: var(--font-code);
    font-size: var(--fs-ui-sm);
  }
  .hint {
    margin: 0;
    color: var(--muted);
    font-size: var(--fs-ui-xs);
  }
  .hint code {
    font-family: var(--font-code);
  }
</style>

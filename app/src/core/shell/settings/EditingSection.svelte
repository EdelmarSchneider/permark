<script lang="ts">
  // Editing: the editor a document opens in, how a reference is written, whether moving a file rewrites its links,
  // the person's own editor for code, and what opening over a shown workspace does. Each change is kept at once; a
  // value the backend refuses is said under its field.
  import type { ApiError, EditModePref, LinkUpdate, OpenIn } from "../../../api";
  import { EDITOR_PRESETS, editorPreview, presetOf } from "../../doc/editorPresets";
  import { DEFAULT_REFERENCE_TEMPLATE, formatReference } from "../../doc/reference";
  import { t, type MessageKey } from "../../i18n";
  import type { WindowStore } from "../../window.svelte";
  import { problemKey, reasonCode } from "./problems";
  import Segments from "./Segments.svelte";
  import SettingRow from "./SettingRow.svelte";

  type Props = { store: WindowStore };
  let { store }: Props = $props();

  const MODES: { value: EditModePref; label: MessageKey }[] = [
    { value: "edit", label: "mode.edit" },
    { value: "source", label: "mode.source" },
  ];
  const LINKS: { value: LinkUpdate; label: MessageKey }[] = [
    { value: "ask", label: "settings.linkAsk" },
    { value: "always", label: "settings.linkAlways" },
    { value: "never", label: "settings.linkNever" },
  ];
  const OPEN_IN: { value: OpenIn; label: MessageKey }[] = [
    { value: "tab", label: "settings.openInTab" },
    { value: "this", label: "settings.openInThis" },
    { value: "window", label: "settings.openInNew" },
  ];

  let templateProblem = $state<ApiError | null>(null);
  let editorProblem = $state<ApiError | null>(null);
  /** The choice the person made in the editor list; until then the one the saved command is. */
  let chosen = $state<string | null>(null);
  const mode = $derived(chosen ?? presetOf(store.editor));

  const preview = $derived(
    formatReference(store.referenceTemplate, { path: "docs/plan.md", section: "scope", line: 12 }),
  );

  async function setTemplate(value: string): Promise<void> {
    templateProblem = await store.trySetting({ key: "referenceTemplate", value });
  }

  async function setEditor(command: string): Promise<void> {
    editorProblem = await store.trySetting({ key: "editor", value: command });
  }

  function pick(id: string): void {
    chosen = id;
    editorProblem = null;
    if (id === "custom") return;
    void setEditor(id === "none" ? "" : (EDITOR_PRESETS.find((p) => p.id === id)?.command ?? ""));
  }
</script>

<SettingRow
  label={t("settings.editMode")}
  labelId="set-edit-mode"
  hint={t("settings.editModeHint")}
>
  <Segments
    labelledby="set-edit-mode"
    value={store.editMode}
    options={MODES.map((o) => ({ value: o.value, label: t(o.label) }))}
    onchange={(value) => void store.changeSetting({ key: "editMode", value })}
  />
</SettingRow>

<SettingRow
  label={t("settings.refTemplate")}
  labelFor="set-reference"
  hint={t("settings.referenceHint")}
  wide
>
  <input
    id="set-reference"
    class="field-input mono"
    type="text"
    spellcheck="false"
    autocomplete="off"
    value={store.referenceTemplate}
    placeholder={DEFAULT_REFERENCE_TEMPLATE}
    aria-invalid={templateProblem !== null}
    onchange={(e) => void setTemplate(e.currentTarget.value)}
  />
  {#snippet note()}
    {#if templateProblem}
      <span class="problem" role="alert">{t(problemKey(reasonCode(templateProblem)))}</span>
    {:else}
      {t("settings.refPreview")} <code>{preview}</code>
    {/if}
  {/snippet}
</SettingRow>

<SettingRow
  label={t("settings.linkUpdate")}
  labelId="set-link-update"
  hint={t("settings.linkUpdateHint")}
>
  <Segments
    labelledby="set-link-update"
    value={store.linkUpdate}
    options={LINKS.map((o) => ({ value: o.value, label: t(o.label) }))}
    onchange={(value) => void store.changeSetting({ key: "linkUpdate", value })}
  />
</SettingRow>

<SettingRow label={t("settings.editor")} labelFor="set-editor" hint={t("settings.editorHint")}>
  <select
    id="set-editor"
    class="field-select"
    value={mode}
    onchange={(e) => pick(e.currentTarget.value)}
  >
    <option value="none">{t("settings.editor.none")}</option>
    {#each EDITOR_PRESETS as preset (preset.id)}
      <option value={preset.id}>{t(preset.label)}</option>
    {/each}
    <option value="custom">{t("settings.editor.custom")}</option>
  </select>
  {#snippet note()}
    {#if mode === "custom"}
      <input
        class="field-input mono full"
        type="text"
        spellcheck="false"
        autocomplete="off"
        aria-label={t("settings.editorCommand")}
        aria-invalid={editorProblem !== null}
        value={store.editor}
        placeholder={t("settings.editorPlaceholder")}
        onchange={(e) => void setEditor(e.currentTarget.value)}
      />
    {/if}
    {#if editorProblem}
      <span class="problem" role="alert">{t(problemKey(reasonCode(editorProblem)))}</span>
    {:else if store.editor.trim() !== ""}
      <span>{t("settings.editorPreview")} <code>{editorPreview(store.editor)}</code></span>
    {/if}
  {/snippet}
</SettingRow>

<SettingRow label={t("settings.openIn")} labelId="set-open-in" hint={t("settings.openInHint")}>
  <Segments
    labelledby="set-open-in"
    value={store.openIn}
    options={OPEN_IN.map((o) => ({ value: o.value, label: t(o.label) }))}
    onchange={(value) => void store.changeSetting({ key: "openIn", value })}
  />
</SettingRow>

<style>
  .mono {
    font-family: var(--font-code);
    font-size: var(--fs-ui-sm);
  }
  .full {
    display: block;
    width: 100%;
    margin-bottom: var(--sp-2);
  }
  code {
    font-family: var(--font-code);
    font-size: var(--fs-ui-xs);
  }
  .problem {
    color: var(--git-deleted-text);
  }
</style>

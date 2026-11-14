<script lang="ts">
  // Editing: the editor a document opens in, how a reference is written, whether moving a file rewrites its links,
  // the person's own editor for code, what opening over a shown workspace does, and the file of symbol groups. Each
  // change is kept at once; a value the backend refuses is said under its field.
  import { onMount } from "svelte";
  import {
    api,
    type ApiError,
    type EditModePref,
    type LinkUpdate,
    type OpenIn,
  } from "../../../api";
  import { EDITOR_PRESETS, editorPreview, presetOf } from "../../doc/editorPresets";
  import { DEFAULT_REFERENCE_TEMPLATE, formatReference } from "../../doc/reference";
  import { apiErrorText } from "../../errors";
  import { t, tn, type MessageKey } from "../../i18n";
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
  let imageFolderProblem = $state<ApiError | null>(null);
  let charactersFileProblem = $state<ApiError | null>(null);
  /** What the last button of the groups file row came to: created the file, or a failure in words. */
  let charactersNews = $state<{ kind: "created" } | { kind: "failed"; text: string } | null>(null);
  let showCharacters = $state<HTMLButtonElement | null>(null);
  /** No groups file yet under a valid name: the row offers to write one with the built-in groups. */
  const canCreate = $derived(
    store.charactersStatus === "builtin" && store.charactersFile.trim() !== "",
  );
  /** The choice the person made in the editor list; until then the one the saved command is. */
  let chosen = $state<string | null>(null);
  const mode = $derived(chosen ?? presetOf(store.editor));

  const preview = $derived(
    formatReference(store.referenceTemplate, { path: "docs/plan.md", section: "scope", line: 12 }),
  );

  async function setTemplate(value: string): Promise<void> {
    templateProblem = await store.trySetting({ key: "referenceTemplate", value });
  }

  async function setImageFolder(value: string): Promise<void> {
    imageFolderProblem = await store.trySetting({ key: "imageFolder", value });
  }

  async function setCharactersFile(value: string): Promise<void> {
    charactersNews = null;
    charactersFileProblem = await store.trySetting({ key: "charactersFile", value });
  }

  async function revealCharacters(): Promise<void> {
    const result = await api.charactersReveal();
    charactersNews = result.ok ? null : { kind: "failed", text: apiErrorText(result.error) };
  }

  async function createCharacters(): Promise<void> {
    const file = store.charactersFile;
    const result = await api.charactersCreate();
    if (!result.ok) {
      charactersNews = { kind: "failed", text: t("settings.charactersCreateFailed", { file }) };
      return;
    }
    if (result.value) {
      charactersNews = { kind: "created" };
      // the Create button goes with the file's arrival: the keyboard moves to the button that stays
      showCharacters?.focus();
    } else {
      // a file appeared since the page read the status: read it
      charactersNews = null;
      store.refreshCharacters();
    }
  }

  // edits to the groups file show in the status line without a restart
  onMount(() => store.refreshCharacters());

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
  label={t("settings.imageFolder")}
  labelFor="set-image-folder"
  hint={t("settings.imageFolderHint")}
  wide
>
  <input
    id="set-image-folder"
    class="field-input"
    type="text"
    spellcheck="false"
    autocomplete="off"
    value={store.imageFolder}
    placeholder="img"
    aria-invalid={imageFolderProblem !== null}
    onchange={(e) => void setImageFolder(e.currentTarget.value)}
  />
  {#snippet note()}
    {#if imageFolderProblem}
      <span class="problem" role="alert">{t(problemKey(reasonCode(imageFolderProblem)))}</span>
    {/if}
  {/snippet}
</SettingRow>

<SettingRow
  label={t("settings.charactersFile")}
  labelFor="set-characters-file"
  hint={t("settings.charactersFileHint")}
  wide
>
  <input
    id="set-characters-file"
    class="field-input"
    type="text"
    spellcheck="false"
    autocomplete="off"
    value={store.charactersFile}
    placeholder="characters.toml"
    aria-invalid={charactersFileProblem !== null}
    onchange={(e) => void setCharactersFile(e.currentTarget.value)}
  />
  {#snippet note()}
    {#if charactersFileProblem}
      <span class="problem" role="alert">{t(problemKey(reasonCode(charactersFileProblem)))}</span>
    {/if}
    <p class="status" aria-live="polite">
      {#if charactersNews?.kind === "failed"}
        <span class="problem">{charactersNews.text}</span>
      {:else if charactersNews?.kind === "created" && store.charactersStatus === "read"}
        {t("settings.charactersCreated", { file: store.charactersFile })}
      {:else if store.charactersStatus === "read"}
        {tn("settings.charactersRead", store.characters.length, { file: store.charactersFile })}
      {:else if store.charactersStatus === "unreadable"}
        <span class="problem"
          >{t("settings.charactersUnreadable", { file: store.charactersFile })}</span
        >
      {:else}
        {t("settings.charactersBuiltin")}
      {/if}
    </p>
    <div class="actions">
      <button
        type="button"
        class="btn btn--sm"
        bind:this={showCharacters}
        onclick={() => void revealCharacters()}>{t("settings.charactersShow")}</button
      >
      {#if canCreate}
        <button type="button" class="btn btn--sm" onclick={() => void createCharacters()}
          >{t("settings.charactersCreate")}</button
        >
      {/if}
    </div>
  {/snippet}
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
  .status {
    margin: 0;
  }
  .actions {
    display: flex;
    flex-wrap: wrap;
    gap: var(--sp-2);
    margin-top: var(--sp-2);
  }
</style>

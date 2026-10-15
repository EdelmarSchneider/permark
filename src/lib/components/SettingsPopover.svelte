<script lang="ts">
  import { onDestroy } from "svelte";
  import { claudeCodeCommand, hookJson, mcpJson } from "../agentSetup";
  import { LINK_UPDATES } from "../fileOps";
  import { EDIT_MODES } from "../editMode";
  import {
    EDITOR_CHOICES,
    previewCommand,
    sampleFile,
    type EditorPresetId,
  } from "../externalEditor";
  import {
    LANGUAGE_NAMES,
    LANGUAGE_PREFERENCES,
    language,
    t,
    type LanguagePreference,
  } from "../i18n";
  import { copyText } from "../platform";
  import { READING_WIDTHS } from "../readingWidth";
  import { DEFAULT_REF_TEMPLATE, formatReference } from "../reference";
  import { agent } from "../stores/agent.svelte";
  import { distribution } from "../stores/distribution.svelte";
  import { docs } from "../stores/docs.svelte";
  import { editorCommand } from "../stores/editorCommand.svelte";
  import { chooseLanguage } from "../stores/languageChoice.svelte";
  import { session } from "../stores/session.svelte";
  import { shell } from "../stores/shell.svelte";
  import { toast } from "../stores/toast.svelte";
  import { ui } from "../stores/ui.svelte";
  import { updates } from "../stores/updates.svelte";
  import { vault } from "../stores/vault.svelte";
  import type { ShellState } from "../types";
  import { describeUpdate, isBusy } from "../updates";
  import IconButton from "./IconButton.svelte";
  import NotesSettings from "./NotesSettings.svelte";
  import Popover, { type Anchor } from "./Popover.svelte";

  const SAMPLE = "docs/specs/live-feed.md";

  // anchor next to whichever settings button is on screen (activity bar, or the header in Simple mode)
  function findAnchor(): Anchor {
    const button = document.querySelector<HTMLElement>("[data-settings-anchor]");
    if (button) return button.getBoundingClientRect();
    return { left: window.innerWidth - 40, right: window.innerWidth - 8, top: 8, bottom: 40 };
  }
  const anchor = findAnchor();
  const simple = anchor.left > window.innerWidth / 2;

  const themes = ["system", "light", "dark"] as const;

  // each language is written in itself, so it can be found whatever the interface says now
  function languageName(choice: LanguagePreference): string {
    return choice === "system" ? t("settings.languageSystem") : LANGUAGE_NAMES[choice];
  }
  const preview = $derived(formatReference(session.refTemplate, docs.path ?? SAMPLE));
  const update = $derived(updates.state);
  // the button already says "Checking..."
  const status = $derived(update.phase === "checking" ? "" : describeUpdate(update));

  // what the editor command would run for the open file, at a sample position
  const editorPreview = $derived(
    previewCommand(
      editorCommand.draft,
      sampleFile(vault.info?.root, docs.path ?? "src/main.rs"),
      42,
      1,
    ),
  );
  // the saved command is read each time the popover opens; what was typed is kept when it closes
  void editorCommand.load();

  // "You're up to date" is the answer to the last check; it is not kept for the next time the popover opens
  onDestroy(() => {
    updates.dismissResult();
    void editorCommand.flush();
  });

  // The file manager entries are read from the system whenever the popover opens and whenever the window regains focus
  // (the person may have picked the default app in Windows Settings meanwhile).
  const fileManager = $derived(shell.state);
  $effect(() => {
    void shell.load();
    const reload = (): void => void shell.load();
    window.addEventListener("focus", reload);
    return () => window.removeEventListener("focus", reload);
  });

  // The lines that connect an agent, each with the program's own path (see `agentSetup.ts`).
  type SetupLine = { id: string; label: string; text: string };
  const setupLines = $derived<SetupLine[]>([
    {
      id: "claude",
      label: t("settings.agentClaude"),
      text: claudeCodeCommand(agent.status.program),
    },
    { id: "json", label: t("settings.agentJson"), text: mcpJson(agent.status.program) },
    { id: "hook", label: t("settings.agentHook"), text: hookJson(agent.status.program) },
  ]);
  let copiedLine = $state<string | null>(null);
  let copiedTimer: ReturnType<typeof setTimeout> | null = null;
  onDestroy(() => {
    if (copiedTimer !== null) clearTimeout(copiedTimer);
  });
  async function copySetup(line: SetupLine): Promise<void> {
    if (!(await copyText(line.text))) {
      toast.message(t("copy.failed"), "warn");
      return;
    }
    copiedLine = line.id;
    if (copiedTimer !== null) clearTimeout(copiedTimer);
    copiedTimer = setTimeout(() => (copiedLine = null), 1500);
  }

  // Writes the change, then shows what the system has: when the write failed the box goes back to where it was.
  async function toggle(
    box: HTMLInputElement,
    change: (enabled: boolean) => Promise<void>,
    actual: (state: ShellState) => boolean,
  ): Promise<void> {
    await change(box.checked);
    if (shell.state) box.checked = actual(shell.state);
  }
</script>

<Popover
  {anchor}
  variant="panel"
  scroll
  side={simple ? "bottom" : "right"}
  align={simple ? "end" : "start"}
  label={t("settings.title")}
  onclose={() => (ui.settingsOpen = false)}
>
  <div class="field">
    <label class="field__label" for="app-language">{t("settings.language")}</label>
    <select
      id="app-language"
      class="text-input select-input"
      value={language.preference}
      onchange={(e) => void chooseLanguage(e.currentTarget.value as LanguagePreference)}
    >
      {#each LANGUAGE_PREFERENCES as choice (choice)}
        <option value={choice} lang={choice === "system" ? undefined : choice}>
          {languageName(choice)}
        </option>
      {/each}
    </select>
  </div>
  <div class="field">
    <span class="field__label">{t("settings.theme")}</span>
    <div class="segmented" role="group" aria-label={t("settings.theme")}>
      {#each themes as theme (theme)}
        <button
          type="button"
          aria-pressed={session.theme === theme}
          onclick={() => session.setTheme(theme)}
        >
          {t(`theme.${theme}`)}
        </button>
      {/each}
    </div>
  </div>
  <div class="field">
    <span class="field__label">{t("settings.width")}</span>
    <div class="segmented" role="group" aria-label={t("settings.width")}>
      {#each READING_WIDTHS as width (width)}
        <button
          type="button"
          aria-pressed={session.width === width}
          onclick={() => session.setWidth(width)}
        >
          {t(`width.${width}`)}
        </button>
      {/each}
    </div>
  </div>
  {#if !vault.isNotes}
    <div class="field">
      <span class="field__label">{t("settings.linkUpdate")}</span>
      <div class="segmented" role="group" aria-label={t("settings.linkUpdate")}>
        {#each LINK_UPDATES as choice (choice)}
          <button
            type="button"
            aria-pressed={session.linkUpdate === choice}
            onclick={() => session.setLinkUpdate(choice)}
          >
            {t(`settings.linkUpdate.${choice}`)}
          </button>
        {/each}
      </div>
    </div>
  {/if}
  <div class="field">
    <span class="field__label">{t("settings.editMode")}</span>
    <div class="segmented" role="group" aria-label={t("settings.editMode")}>
      {#each EDIT_MODES as mode (mode)}
        <button
          type="button"
          aria-pressed={session.editMode === mode}
          onclick={() => session.setEditMode(mode)}
        >
          {t(`edit.${mode}`)}
        </button>
      {/each}
    </div>
  </div>
  <div class="field">
    <div class="field__row">
      <span class="field__label">{t("settings.zoom")}</span>
      <span class="field__row">
        <IconButton
          small
          icon="chevron-down"
          label={t("menu.zoomOut")}
          onclick={() => session.zoomBy(-1)}
        />
        <button
          type="button"
          class="btn btn--sm"
          onclick={() => session.setZoom(1)}
          title={t("status.zoomHint")}
        >
          {Math.round(session.zoom * 100)}%
        </button>
        <IconButton
          small
          icon="chevron-up"
          label={t("menu.zoomIn")}
          onclick={() => session.zoomBy(1)}
        />
      </span>
    </div>
  </div>
  <div class="field">
    <label class="field__label" for="ref-template">{t("settings.refTemplate")}</label>
    <input
      id="ref-template"
      class="text-input"
      value={session.refTemplate}
      placeholder={DEFAULT_REF_TEMPLATE}
      spellcheck="false"
      autocomplete="off"
      oninput={(e) => session.setRefTemplate(e.currentTarget.value)}
    />
    <span class="field__preview" aria-live="polite">{t("settings.refPreview", { preview })}</span>
  </div>
  <div class="field">
    <label class="field__label" for="editor-preset">{t("settings.editor")}</label>
    <select
      id="editor-preset"
      class="text-input select-input"
      value={editorCommand.preset}
      onchange={(e) => editorCommand.pick(e.currentTarget.value as EditorPresetId)}
    >
      {#each EDITOR_CHOICES as choice (choice)}
        <option value={choice}>{t(`settings.editor.${choice}`)}</option>
      {/each}
    </select>
    <input
      id="editor-command"
      class="text-input"
      aria-label={t("settings.editorCommand")}
      aria-describedby="editor-hint"
      aria-invalid={editorCommand.error !== null}
      value={editorCommand.draft}
      placeholder={t("settings.editorPlaceholder")}
      spellcheck="false"
      autocomplete="off"
      oninput={(e) => editorCommand.type(e.currentTarget.value)}
    />
    {#if editorCommand.error}
      <span class="field__status field__status--error" role="alert">{editorCommand.error}</span>
    {:else if editorPreview !== ""}
      <span class="field__preview" aria-live="polite"
        >{t("settings.editorPreview", { preview: editorPreview })}</span
      >
    {/if}
    <span id="editor-hint" class="field__status">{t("settings.editorHint")}</span>
  </div>
  {#if fileManager?.supported}
    <div class="field">
      <span class="field__label">{t("settings.fileManager")}</span>
      <label class="filter__check">
        <input
          type="checkbox"
          checked={fileManager.contextMenu}
          disabled={shell.busy}
          aria-describedby="shell-menu-hint"
          onchange={(e) =>
            void toggle(
              e.currentTarget,
              (on) => shell.setContextMenu(on),
              (state) => state.contextMenu,
            )}
        />
        <span class="filter__text">
          <span>{t("settings.shellMenu")}</span>
          <span id="shell-menu-hint" class="filter__hint">{t("settings.shellMenuHint")}</span>
        </span>
      </label>
      <label class="filter__check">
        <input
          type="checkbox"
          checked={fileManager.association}
          disabled={shell.busy}
          aria-describedby="shell-assoc-hint"
          onchange={(e) =>
            void toggle(
              e.currentTarget,
              (on) => shell.setAssociation(on),
              (state) => state.association,
            )}
        />
        <span class="filter__text">
          <span>{t("settings.shellAssoc")}</span>
          <span id="shell-assoc-hint" class="filter__hint">{t("settings.shellAssocHint")}</span>
        </span>
      </label>
      {#if fileManager.association}
        {#if fileManager.isDefault}
          <span class="field__status" role="status">{t("settings.shellIsDefault")}</span>
        {:else}
          <div class="field__row">
            <button type="button" class="btn btn--sm" onclick={() => void shell.makeDefault()}>
              {t("settings.shellDefault")}
            </button>
          </div>
          <span class="field__status">{t("settings.shellDefaultHint")}</span>
        {/if}
      {/if}
    </div>
  {/if}
  <NotesSettings />
  {#if !vault.isNotes}
    <div class="field">
      <span class="field__label">{t("settings.agent")}</span>
      <label class="filter__check">
        <input
          type="checkbox"
          checked={agent.enabled}
          disabled={agent.busy || vault.isSimple}
          aria-describedby="agent-hint"
          onchange={async (e) => {
            const box = e.currentTarget;
            await agent.setEnabled(box.checked);
            // when the change did not go through the box goes back to what is true
            box.checked = agent.enabled;
          }}
        />
        <span class="filter__text">
          <span>{t("settings.agentSwitch")}</span>
          <span id="agent-hint" class="filter__hint"
            >{vault.isSimple ? t("settings.agentUnavailable") : t("settings.agentHint")}</span
          >
        </span>
      </label>
      {#if agent.enabled}
        <div class="agent-setup">
          <span class="field__label">{t("settings.agentSetup")}</span>
          {#each setupLines as line (line.id)}
            <div class="agent-setup__head">
              <span>{line.label}</span>
              <button type="button" class="btn btn--sm" onclick={() => void copySetup(line)}>
                {copiedLine === line.id ? t("common.copied") : t("common.copy")}
              </button>
            </div>
            <textarea
              class="agent-code"
              readonly
              wrap="off"
              spellcheck="false"
              rows={line.text.split("\n").length}
              aria-label={line.label}
              value={line.text}></textarea>
          {/each}
          <span class="field__status"
            >{distribution.isStore
              ? t("settings.agentSetupHintStore")
              : t("settings.agentSetupHint")}</span
          >
        </div>
      {/if}
    </div>
  {/if}
  <div class="field">
    <span class="field__label">{t("settings.updates")}</span>
    {#if distribution.isStore}
      <span class="field__status">{t("settings.updatesStore")}</span>
    {:else}
      <div class="field__row">
        {#if update.phase === "available"}
          <button
            type="button"
            class="btn btn--sm btn--primary"
            onclick={() => void updates.install()}
          >
            {t("update.restart")}
          </button>
        {:else}
          <button
            type="button"
            class="btn btn--sm"
            disabled={isBusy(update)}
            onclick={() => void updates.check(true)}
          >
            {update.phase === "checking" ? t("update.checking") : t("update.check")}
          </button>
        {/if}
      </div>
      <span class="field__status" role="status">{status}</span>
    {/if}
  </div>
</Popover>

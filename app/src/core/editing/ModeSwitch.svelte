<script lang="ts">
  // View | Edit | Source, Save and the unsaved dot. Edit only for Markdown; a document that cannot be edited
  // says why.
  import Workflow from "@lucide/svelte/icons/workflow";
  import { t, type MessageKey } from "../i18n";
  import { diagramPanel } from "./diagramPanel.svelte";
  import type { EditMode, EditingSession } from "./session.svelte";

  type Props = {
    session: EditingSession;
    /** Why the document cannot be edited (`null`: it can). */
    readOnly: string | null;
    /** The document has an Edit mode (Markdown | a sheet). */
    visual: boolean;
    choose: (mode: EditMode) => void;
  };
  let { session, readOnly, visual, choose }: Props = $props();

  const WHY: Readonly<Record<string, MessageKey>> = {
    binary: "edit.disabled.binary",
    tooLarge: "edit.disabled.tooLarge",
    lossy: "edit.disabled.lossy",
    media: "edit.disabled.media",
    noViewer: "edit.disabled.noViewer",
  };

  const why = $derived(readOnly === null ? null : t(WHY[readOnly] ?? "edit.disabled.noViewer"));
  const editing = $derived(session.mode !== "view");
</script>

<div class="modes" role="group" aria-label={t("mode.label")}>
  <button
    type="button"
    class="mode"
    aria-pressed={session.mode === "view"}
    title={t("mode.viewHint")}
    onclick={() => choose("view")}
  >
    {t("mode.view")}
  </button>
  <button
    type="button"
    class="mode"
    aria-pressed={session.mode === "edit"}
    disabled={why !== null || !visual}
    title={why ?? (visual ? t("mode.editHint") : t("edit.visualForMarkdown"))}
    onclick={() => choose("edit")}
  >
    {t("mode.edit")}
  </button>
  <button
    type="button"
    class="mode"
    aria-pressed={session.mode === "source"}
    disabled={why !== null}
    title={why ?? t("mode.sourceHint")}
    onclick={() => choose("source")}
  >
    {t("mode.source")}
  </button>
</div>
{#if session.mode === "source" && session.kind === "markdown"}
  <button
    type="button"
    class="icon-btn"
    aria-pressed={diagramPanel.on}
    title={t("edit.diagramPanelHint")}
    aria-label={t("edit.diagramPanel")}
    onclick={() => diagramPanel.toggle()}
  >
    <Workflow size={15} />
  </button>
{/if}
{#if editing && session.policy.kind === "explicit"}
  <button
    type="button"
    class="btn btn--sm"
    class:btn--primary={session.unsaved}
    disabled={!session.unsaved}
    title={t("edit.saveHint")}
    onclick={() => session.save()}
  >
    {#if session.unsaved}<span class="dot" aria-hidden="true"></span>{/if}
    {session.state.s === "saving" ? t("edit.saving") : t("edit.save")}
  </button>
{/if}

<style>
  .modes {
    display: inline-flex;
    padding: 2px;
    border: 1px solid var(--border);
    border-radius: var(--radius-control);
    background: var(--surface);
  }
  .mode {
    height: 22px;
    padding: 0 8px;
    border: none;
    border-radius: 4px;
    background: transparent;
    color: var(--muted);
    font-size: var(--fs-ui-sm);
    cursor: pointer;
  }
  .mode[aria-pressed="true"] {
    background: var(--bg);
    color: var(--text);
    box-shadow: 0 1px 2px rgb(0 0 0 / 0.08);
  }
  .mode:disabled {
    opacity: 0.45;
    cursor: default;
  }
  .dot {
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background: currentColor;
  }
  .icon-btn[aria-pressed="true"] {
    background: var(--accent-soft);
    color: var(--accent-text);
  }
</style>

<script lang="ts">
  // View | Edit | Source, Save and the unsaved dot. Edit only for Markdown; a document that cannot be edited
  // says why. `compact` (a narrow header): icons only, the words stay as label + tooltip.
  import Code from "@lucide/svelte/icons/code";
  import Eye from "@lucide/svelte/icons/eye";
  import Pencil from "@lucide/svelte/icons/pencil";
  import Save from "@lucide/svelte/icons/save";
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
    compact?: boolean;
  };
  let { session, readOnly, visual, choose, compact = false }: Props = $props();

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

<div class="modes" class:compact role="group" aria-label={t("mode.label")}>
  <button
    type="button"
    class="mode"
    aria-pressed={session.mode === "view"}
    aria-label={compact ? t("mode.view") : undefined}
    title={t("mode.viewHint")}
    onclick={() => choose("view")}
  >
    {#if compact}<Eye size={14} />{:else}{t("mode.view")}{/if}
  </button>
  <button
    type="button"
    class="mode"
    aria-pressed={session.mode === "edit"}
    aria-label={compact ? t("mode.edit") : undefined}
    disabled={why !== null || !visual}
    title={why ?? (visual ? t("mode.editHint") : t("edit.visualForMarkdown"))}
    onclick={() => choose("edit")}
  >
    {#if compact}<Pencil size={14} />{:else}{t("mode.edit")}{/if}
  </button>
  <button
    type="button"
    class="mode"
    aria-pressed={session.mode === "source"}
    aria-label={compact ? t("mode.source") : undefined}
    disabled={why !== null}
    title={why ?? t("mode.sourceHint")}
    onclick={() => choose("source")}
  >
    {#if compact}<Code size={14} />{:else}{t("mode.source")}{/if}
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
  {@const label = session.state.s === "saving" ? t("edit.saving") : t("edit.save")}
  <button
    type="button"
    class="btn btn--sm save"
    class:compact
    class:btn--primary={session.unsaved}
    disabled={!session.unsaved}
    aria-label={compact ? label : undefined}
    title={t("edit.saveHint")}
    onclick={() => session.save()}
  >
    {#if session.unsaved}<span class="dot" aria-hidden="true"></span>{/if}
    {#if compact}<Save size={14} />{:else}{label}{/if}
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
  .modes,
  .save {
    flex: none;
  }
  .mode {
    display: inline-grid;
    place-items: center;
    height: 22px;
    padding: 0 8px;
    border: none;
    border-radius: 4px;
    background: transparent;
    color: var(--muted);
    font-size: var(--fs-ui-sm);
    cursor: pointer;
  }
  .compact .mode {
    width: 26px;
    padding: 0;
  }
  .save.compact {
    gap: 3px;
    padding-inline: 6px;
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

<script lang="ts" module>
  import { conditions } from "../conditions/context.svelte";
  import { contributions } from "../plugins/instance.svelte";
  import type { ContributionOf } from "../plugins/registry.svelte";
  import type { EditMode } from "./session.svelte";

  /** The plugins' edit actions shown with the mode switch in `mode` (none in View); the header counts them. */
  export function shownEditActions(mode: EditMode): ContributionOf<"editAction">[] {
    if (mode === "view") return [];
    return contributions
      .visible("editAction", conditions.current)
      .filter((action) => action.when?.() ?? true);
  }
</script>

<script lang="ts">
  // View | Edit | Source, Save and the unsaved dot. Edit only for Markdown; a document that cannot be edited
  // says why. `compact` (a narrow header): icons only, the words stay as label + tooltip. While editing, the
  // plugins' edit actions (icon buttons, each opening its menu below it) come after the diagram toggle.
  import Code from "@lucide/svelte/icons/code";
  import Eye from "@lucide/svelte/icons/eye";
  import Pencil from "@lucide/svelte/icons/pencil";
  import Save from "@lucide/svelte/icons/save";
  import Workflow from "@lucide/svelte/icons/workflow";
  import { keyHint } from "../commands/hint";
  import ContextMenu from "../explorer/ContextMenu.svelte";
  import type { MenuEntry } from "../explorer/menu";
  import { t, type MessageKey } from "../i18n";
  import { diagramPanel } from "./diagramPanel.svelte";
  import type { EditingSession } from "./session.svelte";

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
  const actions = $derived(shownEditActions(session.mode));

  type Action = ContributionOf<"editAction">;
  type Open = {
    key: string;
    x: number;
    y: number;
    anchor: HTMLElement;
    label: string;
    entries: MenuEntry[];
    /** The keyboard starts on the first item that can act now. */
    start: number;
  };
  let menu = $state.raw<Open | null>(null);
  /** The open menu while its action is still shown (back in View: gone). */
  const shownMenu = $derived(
    menu !== null && actions.some((action) => keyOf(action) === menu?.key) ? menu : null,
  );

  function keyOf(action: Action): string {
    return action.owner + action.id;
  }

  function tip(action: Action): string {
    const keys = action.command === undefined ? null : keyHint(action.command);
    return keys === null ? t(action.label) : `${t(action.label)} (${keys})`;
  }

  /** Opens the action's menu below its button; a second press closes it. */
  function toggle(action: Action, button: HTMLElement): void {
    const key = keyOf(action);
    if (shownMenu?.key === key) {
      menu = null;
      return;
    }
    const box = button.getBoundingClientRect();
    const entries = action.entries();
    menu = {
      key,
      x: box.left,
      y: box.bottom + 4,
      anchor: button,
      label: t(action.label),
      entries,
      start: entries.findIndex((entry) => entry.kind === "item" && entry.disabled !== true),
    };
  }
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
{#each actions as action (keyOf(action))}
  {@const Icon = action.icon}
  <button
    type="button"
    class="icon-btn"
    title={tip(action)}
    aria-label={t(action.label)}
    aria-haspopup="menu"
    aria-expanded={shownMenu?.key === keyOf(action)}
    onclick={(event) => toggle(action, event.currentTarget)}
  >
    <Icon size={15} />
  </button>
{/each}
{#if shownMenu}
  <ContextMenu
    x={shownMenu.x}
    y={shownMenu.y}
    entries={shownMenu.entries}
    close={() => (menu = null)}
    label={shownMenu.label}
    anchor={shownMenu.anchor}
    start={shownMenu.start}
  />
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

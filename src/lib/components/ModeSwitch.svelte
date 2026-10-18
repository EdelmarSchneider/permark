<script lang="ts">
  // The header's mode switch: View | Edit | Source (docs/design/v0.2.md section 3). It is always there while a document is open:
  // one click goes to any mode, and a document that cannot be edited keeps View with the other two off and the reason as their tooltip.
  import { DOC_MODES, focusTarget, shortcutToward, type DocMode } from "../editMode";
  import { keys, t, type MessageKey } from "../i18n";
  import { edit } from "../stores/edit.svelte";
  import { toast } from "../stores/toast.svelte";
  import Icon, { type IconName } from "./Icon.svelte";

  const LABELS: Record<DocMode, MessageKey> = {
    read: "mode.read",
    visual: "edit.visual",
    source: "edit.source",
  };
  /** At narrow widths the labels give way to these. */
  const ICONS: Record<DocMode, IconName> = { read: "book-open", visual: "pencil", source: "code" };

  let group = $state<HTMLElement>();

  const current = $derived<DocMode>(edit.active ? edit.mode : "read");
  // why a mode is off: the document cannot be edited at all, or (a plain text file) Edit is for Markdown
  const blocked = $derived(edit.blocked);
  const visualBlocked = $derived(blocked ?? (edit.sourceOnly ? t("edit.visualForMarkdown") : null));

  function reason(mode: DocMode): string | null {
    if (mode === "read") return null;
    return mode === "visual" ? visualBlocked : blocked;
  }

  function isOff(mode: DocMode): boolean {
    return reason(mode) !== null;
  }

  function title(mode: DocMode): string {
    const why = reason(mode);
    if (why) return why;
    const hint = t(`mode.${mode}Hint`);
    const shortcut = shortcutToward(mode, current, edit.lastEditMode);
    return shortcut ? t("mode.withKeys", { hint, keys: keys(shortcut) }) : hint;
  }

  function choose(mode: DocMode): void {
    if (mode === current || edit.switching || edit.loading) return;
    const why = reason(mode);
    if (why) {
      // the segment stays reachable (so its tooltip can say why), and a click says it too
      toast.message(why, "info");
      return;
    }
    void edit.goTo(mode);
  }

  /** Arrow keys, Home and End move the focus along the segments; Enter and Space choose the one that has it. */
  function onkeydown(e: KeyboardEvent, mode: DocMode): void {
    if (e.ctrlKey || e.metaKey || e.altKey || e.shiftKey) return;
    const to = focusTarget(mode, e.key);
    if (!to) return;
    e.preventDefault();
    group?.querySelector<HTMLElement>(`[data-mode="${to}"]`)?.focus();
  }
</script>

<div
  class="seg"
  role="radiogroup"
  aria-label={t("mode.label")}
  aria-busy={edit.switching || edit.loading}
  bind:this={group}
>
  {#each DOC_MODES as mode (mode)}
    <button
      type="button"
      role="radio"
      class={["seg__btn", current === mode && "is-on", isOff(mode) && "is-disabled"]}
      data-mode={mode}
      aria-checked={current === mode}
      aria-disabled={isOff(mode)}
      aria-label={t(LABELS[mode])}
      tabindex={current === mode ? 0 : -1}
      title={title(mode)}
      onclick={() => choose(mode)}
      onkeydown={(e) => onkeydown(e, mode)}
    >
      <Icon name={ICONS[mode]} size="sm" class="seg__icon" />
      <span class="seg__label">{t(LABELS[mode])}</span>
    </button>
  {/each}
</div>

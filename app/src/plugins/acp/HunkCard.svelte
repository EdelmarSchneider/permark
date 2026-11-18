<script lang="ts">
  // One run of lines the agent changed in its copy: where it sits in the real file, the removed lines in red and the
  // new ones in green (unchanged lines between them kept to one line of context), and Accept | Reject for this run
  // alone. A file that is accepted | rejected only as a whole (new, deleted) shows its lines without the buttons.
  import type { HunkView } from "../../api";
  import DiffLines from "../../core/editing/DiffLines.svelte";
  import { diffRows, type DiffRow } from "../../core/editing/diffRows";
  import { t } from "../../core/i18n";

  type Props = {
    hunk: HunkView;
    /** The run stands for the whole file: no place, no buttons of its own. */
    whole?: boolean;
    /** An accept | reject is running: the buttons wait. */
    disabled?: boolean;
    onaccept: () => void;
    onreject: () => void;
  };
  let { hunk, whole = false, disabled = false, onaccept, onreject }: Props = $props();

  const uid = $props.id();

  const where = $derived(
    hunk.oldLines.length === 0
      ? t("acp.hunkBefore", { line: hunk.oldStart })
      : hunk.oldLines.length === 1
        ? t("acp.hunkLine", { line: hunk.oldStart })
        : t("acp.hunkLines", {
            from: hunk.oldStart,
            to: hunk.oldStart + hunk.oldLines.length - 1,
          }),
  );

  /** Only new | only removed lines are drawn as they are (a diff of an empty side would show a blank line). */
  const rows = $derived.by((): DiffRow[] => {
    if (hunk.oldLines.length === 0)
      return hunk.newLines.map((text, i) => ({ kind: "add", text, key: `a${String(i)}` }));
    if (hunk.newLines.length === 0)
      return hunk.oldLines.map((text, i) => ({ kind: "del", text, key: `d${String(i)}` }));
    return diffRows(hunk.oldLines.join("\n"), hunk.newLines.join("\n"), 1);
  });
</script>

<div class="hunk" role="group" aria-labelledby={whole ? undefined : `${uid}-where`}>
  {#if !whole}
    <div class="head">
      <span class="where" id="{uid}-where">{where}</span>
      <span class="spacer"></span>
      <button
        type="button"
        class="btn btn--sm"
        aria-describedby="{uid}-where"
        {disabled}
        onclick={onreject}>{t("acp.reject")}</button
      >
      <button
        type="button"
        class="btn btn--sm btn--primary"
        aria-describedby="{uid}-where"
        {disabled}
        onclick={onaccept}>{t("acp.accept")}</button
      >
    </div>
  {/if}
  <div class="diff">
    <DiffLines {rows} />
  </div>
</div>

<style>
  .hunk {
    display: flex;
    flex-direction: column;
    border: 1px solid var(--border);
    border-radius: var(--radius-control);
    background: var(--bg);
    overflow: hidden;
  }
  .head {
    display: flex;
    align-items: center;
    gap: var(--sp-1);
    padding: var(--sp-1) var(--sp-2);
    border-bottom: 1px solid var(--border);
    background: var(--surface);
  }
  .where {
    min-width: 0;
    overflow: hidden;
    color: var(--muted);
    font-size: var(--fs-ui-xs);
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .spacer {
    flex: 1;
  }
  .diff {
    font-family: var(--font-code);
    font-size: var(--fs-ui-sm);
  }
</style>

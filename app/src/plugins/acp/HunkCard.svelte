<script lang="ts">
  // One run of lines the agent changed in its copy: where it sits in the real file (Open shows the file there), the
  // removed lines in red and the new ones in green between the real line above and below them (unchanged lines muted,
  // those between changes kept to one line of context), and
  // Accept | Reject for this run alone. A file that is accepted | rejected only as a whole (new, deleted) shows its
  // lines without the buttons. A long run draws its first `MAX_ROWS` rows until the person asks for all; a run the
  // backend cut says so (a choice still takes all of it).
  import SquareArrowOutUpRight from "@lucide/svelte/icons/square-arrow-out-up-right";
  import type { HunkView } from "../../api";
  import DiffLines from "../../core/editing/DiffLines.svelte";
  import { diffRows, type DiffRow } from "../../core/editing/diffRows";
  import { t } from "../../core/i18n";
  import Note from "./Note.svelte";

  type Props = {
    hunk: HunkView;
    /** The project path of the file. */
    path: string;
    /** The run stands for the whole file: no place, no buttons of its own. */
    whole?: boolean;
    /** An accept | reject is running: the buttons wait. */
    disabled?: boolean;
    onaccept: () => void;
    onreject: () => void;
    /** Shows the real file at the run's line. */
    onopen: () => void;
  };
  let { hunk, path, whole = false, disabled = false, onaccept, onreject, onopen }: Props = $props();

  /** Rows drawn before "Show all": a whole new file of thousands of lines would freeze the panel. */
  const MAX_ROWS = 200;
  const uid = $props.id();
  let all = $state(false);

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
  const shown = $derived(all || rows.length <= MAX_ROWS ? rows : rows.slice(0, MAX_ROWS));
  /** The run between the real file's line above and below it (a whole file has none; a cut run ends where cut). */
  const drawn = $derived.by((): DiffRow[] => {
    if (whole) return shown;
    const out: DiffRow[] = [];
    if (hunk.contextBefore !== null)
      out.push({ kind: "same", text: hunk.contextBefore, key: "context-before" });
    out.push(...shown);
    if (hunk.contextAfter !== null && shown.length === rows.length)
      out.push({ kind: "same", text: hunk.contextAfter, key: "context-after" });
    return out;
  });
</script>

<div class="hunk" role="group" aria-label={whole ? undefined : t("acp.hunkGroup", { path, where })}>
  {#if !whole}
    <div class="head">
      <span class="where" id="{uid}-where">{where}</span>
      <span class="spacer"></span>
      <button
        type="button"
        class="icon"
        aria-label={t("acp.openAt", { path, line: hunk.oldStart })}
        title={t("acp.openAt", { path, line: hunk.oldStart })}
        onclick={onopen}
      >
        <SquareArrowOutUpRight size={13} aria-hidden="true" />
      </button>
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
    <DiffLines rows={drawn} />
  </div>
  {#if shown.length < rows.length}
    <button type="button" class="btn btn--sm btn--ghost more" onclick={() => (all = true)}
      >{t("acp.showAllLines", { n: rows.length })}</button
    >
  {/if}
  {#if hunk.truncated}
    <div class="cut"><Note>{t("acp.truncated")}</Note></div>
  {/if}
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
  /* a narrow panel puts the buttons under the place: the place is never cut to "Li…" */
  .head {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--sp-1);
    padding: var(--sp-1) var(--sp-2);
    border-bottom: 1px solid var(--border);
    background: var(--surface);
  }
  .where {
    color: var(--muted);
    font-size: var(--fs-ui-sm);
    white-space: nowrap;
  }
  .spacer {
    flex: 1;
  }
  .icon {
    flex: none;
    display: inline-grid;
    place-items: center;
    width: var(--icon-btn-sm);
    height: var(--icon-btn-sm);
    border: 0;
    border-radius: var(--radius-control);
    background: transparent;
    color: var(--muted);
    cursor: pointer;
  }
  .icon:hover {
    background: var(--hover);
    color: var(--text);
  }
  .diff {
    font-family: var(--font-code);
    font-size: var(--fs-ui-sm);
  }
  /* unchanged lines only place the change: they step back */
  .diff :global(.row.same) {
    color: var(--muted);
  }
  .more {
    align-self: center;
    margin: var(--sp-1);
  }
  .cut {
    padding: var(--sp-1) var(--sp-2);
    border-top: 1px solid var(--border);
  }
</style>

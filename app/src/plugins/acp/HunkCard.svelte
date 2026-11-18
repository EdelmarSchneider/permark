<script lang="ts">
  // One run of lines the agent changed in its copy: where it sits in the real file (Open shows the file there), the
  // removed lines in red and the new ones in green between the real line above and below them (a blank one places
  // nothing and is left out; unchanged lines muted, those between changes kept to one line of context), and
  // Accept | Reject for this run alone. A file that is accepted | rejected only as a whole (new, deleted) shows its
  // lines without the buttons. A long run draws its first `MAX_ROWS` rows until the person asks for all; a run the
  // backend cut says so (a choice still takes all of it).
  import SquareArrowOutUpRight from "@lucide/svelte/icons/square-arrow-out-up-right";
  import type { HunkView } from "../../api";
  import { runRows, type DiffRow } from "../../core/editing/diffRows";
  import { t } from "../../core/i18n";
  import DiffBox from "./DiffBox.svelte";
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

  const rows = $derived(runRows(hunk.oldLines, hunk.newLines, 1));
  const shown = $derived(all || rows.length <= MAX_ROWS ? rows : rows.slice(0, MAX_ROWS));
  /** The run between the real file's line above and below it (a whole file has none; a cut run ends where cut). */
  const drawn = $derived.by((): DiffRow[] => {
    if (whole) return shown;
    const out: DiffRow[] = [];
    if (placing(hunk.contextBefore))
      out.push({ kind: "same", text: hunk.contextBefore, key: "context-before" });
    out.push(...shown);
    if (placing(hunk.contextAfter) && shown.length === rows.length)
      out.push({ kind: "same", text: hunk.contextAfter, key: "context-after" });
    return out;
  });

  /** A real line that helps find the run: a blank one would draw an empty row at its edge. */
  function placing(line: string | null): line is string {
    return line !== null && line.trim() !== "";
  }
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
  <DiffBox rows={drawn} />
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
  .more {
    align-self: center;
    margin: var(--sp-1);
  }
  .cut {
    padding: var(--sp-1) var(--sp-2);
    border-top: 1px solid var(--border);
  }
</style>

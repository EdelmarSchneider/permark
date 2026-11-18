<script lang="ts">
  // The rows of `diffRows` drawn: removed lines on red with a minus, new ones on green with a plus, a folded run of
  // unchanged lines as its count. The frame around them (border, scroll, code font and size) is the caller's.
  import { t } from "../i18n";
  import type { DiffRow } from "./diffRows";

  type Props = { rows: readonly DiffRow[] };
  let { rows }: Props = $props();
</script>

{#each rows as row (row.key)}
  {#if row.kind === "fold"}
    <div class="row fold">{t("conflict.folded", { count: row.count })}</div>
  {:else}
    <div class={["row", row.kind]}>
      <span class="sign" aria-hidden="true"
        >{row.kind === "add" ? "+" : row.kind === "del" ? "−" : ""}</span
      >
      <span class="text">{row.text === "" ? " " : row.text}</span>
    </div>
  {/if}
{/each}

<style>
  .row {
    display: flex;
    gap: var(--sp-2);
    padding: 0 var(--sp-2);
    white-space: pre-wrap;
    overflow-wrap: anywhere;
  }
  .sign {
    flex: none;
    width: 1ch;
    color: var(--faint);
  }
  .add {
    background: var(--review-add-bg);
  }
  .add .sign {
    color: var(--git-added-text);
  }
  .del {
    background: var(--review-del-bg);
  }
  .del .sign {
    color: var(--git-deleted-text);
  }
  .fold {
    justify-content: center;
    color: var(--faint);
    font-family: var(--font-ui);
    font-size: var(--fs-ui-xs);
  }
</style>

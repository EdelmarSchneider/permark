<script lang="ts">
  // One row of the settings page: what it is (label + hint) at the left, the control at the right. Stacks when the settings
  // pane is narrow, the control goes under the label when both do not fit side by side, and always when `wide` (a long control: a text field, a list). Rows of one card are separated by a line. A folder
  // setting says who it is for (`scope`) with a chip in its label. An app row whose key the folder's shared file also
  // sets (`ignored`, decided by the caller) says, in a quiet line, that the file's value is not used.
  import type { Snippet } from "svelte";
  import { t } from "../../i18n";
  import ScopeChip from "./ScopeChip.svelte";

  type Props = {
    label: string;
    hint?: string;
    /** The id of the control the label names (a `<label for>`), else the label is plain text with `labelId`. */
    labelFor?: string;
    labelId?: string;
    /** The control takes the row's full width under the label. */
    wide?: boolean;
    /** Who the value is for: everyone sharing the folder (its config file) or only this person. App rows: none. */
    scope?: "everyone" | "you";
    children: Snippet;
    /** A line under the control (a problem, a preview). */
    note?: Snippet;
    /** The folder's shared file also sets this row's key, and that value is not used. */
    ignored?: boolean;
  };
  let {
    label,
    hint,
    labelFor,
    labelId,
    wide = false,
    scope,
    children,
    note,
    ignored = false,
  }: Props = $props();
</script>

<div class="row" class:wide>
  <div class="text">
    {#if labelFor}
      <label class="name" for={labelFor}
        >{label}{#if scope}{" "}<ScopeChip {scope} />{/if}</label
      >
    {:else}
      <span class="name" id={labelId}
        >{label}{#if scope}{" "}<ScopeChip {scope} />{/if}</span
      >
    {/if}
    {#if hint}<span class="hint">{hint}</span>{/if}
  </div>
  <div class="control">{@render children()}</div>
  {#if note || ignored}
    <div class="note">
      {#if note}{@render note()}{/if}
      {#if ignored}<span>{t("settings.ignoredKey")}</span>{/if}
    </div>
  {/if}
</div>

<style>
  /* a wrapping line, not two fixed columns: a control too wide to sit beside a readable label (a long segmented
     choice, a long translation) goes under it instead of squeezing the label into a narrow column */
  .row {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    column-gap: var(--sp-5);
    row-gap: var(--sp-2);
    padding: var(--sp-3) var(--sp-4);
  }
  .text {
    display: flex;
    flex: 1 1 16rem;
    flex-direction: column;
    gap: 2px;
    min-width: 0;
  }
  .name {
    font-size: var(--fs-ui);
    font-weight: 500;
  }
  .hint {
    color: var(--muted);
    font-size: var(--fs-ui-sm);
    overflow-wrap: anywhere;
  }
  .control {
    display: flex;
    flex: 0 1 auto;
    align-items: center;
    justify-content: flex-end;
    flex-wrap: wrap;
    gap: var(--sp-2);
    min-width: 0;
    max-width: 100%;
  }
  .wide .text,
  .wide .control,
  .note {
    flex-basis: 100%;
  }
  .wide .control {
    justify-content: stretch;
  }
  .wide .control > :global(input),
  .wide .control > :global(select) {
    flex: 1;
    min-width: 0;
  }
  .note {
    color: var(--muted);
    font-size: var(--fs-ui-sm);
    overflow-wrap: anywhere;
  }
  /* stacks by the width of the settings pane (beside its section list), not of the window */
  @container settings (max-width: 600px) {
    .text,
    .control {
      flex-basis: 100%;
    }
    .control {
      justify-content: flex-start;
    }
  }
  /* web views without container queries (WebKitGTK before 2.40): the window's width */
  @media (max-width: 640px) {
    .text,
    .control {
      flex-basis: 100%;
    }
    .control {
      justify-content: flex-start;
    }
  }
</style>

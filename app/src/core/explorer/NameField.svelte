<script lang="ts">
  // The inline name field of a rename | new entry: focused with the name (up to its extension) selected; Enter
  // applies, Escape | leaving the field cancels; what is wrong with the name shows under it. How a new file is named
  // stays inside the row (the rows below are never covered): the `suffix` added after a name without an extension,
  // the `note` in the field's description and tooltip.
  import { untrack } from "svelte";
  import { stemLength } from "./list";

  type Props = {
    value: string;
    label: string;
    hint: string;
    problem: string | null;
    /** How the name is used (`null`: nothing to say): the field's description and tooltip. */
    note?: string | null;
    /** Shown after a name typed without an extension: what is added to it (`null`: nothing). */
    suffix?: string | null;
    oninput: (value: string) => void;
    commit: () => void;
    cancel: () => void;
  };
  let {
    value,
    label,
    hint,
    problem,
    note = null,
    suffix = null,
    oninput,
    commit,
    cancel,
  }: Props = $props();

  const noteId = $props.id();
  const hasNote = $derived(note !== null && note !== "");
  // no extension typed (yet): the suffix is what the name gets
  const added = $derived(suffix !== null && stemLength(value) === value.length);

  let input: HTMLInputElement | undefined = $state();

  // focus once when shown (the typed text must not re-select itself)
  $effect(() => {
    if (!input) return;
    input.focus();
    untrack(() => input?.setSelectionRange(0, stemLength(value)));
  });

  function onkeydown(e: KeyboardEvent): void {
    if (e.isComposing) return;
    e.stopPropagation();
    if (e.key === "Enter") {
      e.preventDefault();
      commit();
    } else if (e.key === "Escape") {
      e.preventDefault();
      cancel();
    }
  }
</script>

<span class="field">
  <input
    bind:this={input}
    type="text"
    spellcheck="false"
    autocomplete="off"
    aria-label={label}
    aria-invalid={problem !== null}
    aria-describedby={hasNote && !problem ? noteId : undefined}
    title={hasNote ? `${hint}\n${note}` : hint}
    {value}
    oninput={(e) => oninput(e.currentTarget.value)}
    onblur={cancel}
    {onkeydown}
    onclick={(e) => e.stopPropagation()}
    ondblclick={(e) => e.stopPropagation()}
    onpointerdown={(e) => e.stopPropagation()}
  />
  {#if added}
    <span class="suffix" aria-hidden="true">{suffix}</span>
  {/if}
  {#if hasNote && !problem}
    <!-- read with the field; a problem with the name takes its place -->
    <span class="note sr-only" id={noteId}>{note}</span>
  {/if}
  {#if problem}
    <span class="problem" role="alert">{problem}</span>
  {/if}
</span>

<style>
  .field {
    position: relative;
    display: flex;
    align-items: center;
    gap: 2px;
    flex: 1;
    min-width: 0;
  }
  input {
    flex: 1;
    min-width: 0;
    width: 100%;
    height: 22px;
    padding: 0 4px;
    border: 1px solid var(--accent);
    border-radius: 4px;
    background: var(--bg);
    color: var(--text);
    font: inherit;
    font-size: var(--fs-ui);
  }
  input[aria-invalid="true"] {
    border-color: var(--git-deleted);
  }
  .problem {
    position: absolute;
    top: 24px;
    left: 0;
    z-index: 5;
    max-width: 280px;
    padding: 3px 8px;
    border: 1px solid var(--git-deleted);
    border-radius: 4px;
    background: var(--bg);
    color: var(--git-deleted-text);
    font-size: var(--fs-ui-xs);
    box-shadow: var(--shadow-pop);
  }
  .suffix {
    flex: none;
    color: var(--muted);
    font-size: var(--fs-ui-sm);
  }
</style>

<script lang="ts" generics="V extends string">
  // A row of exclusive choices (a radio group drawn as segments). Arrows move the choice (and the focus) like any
  // radio group.
  import { tick } from "svelte";

  type Option = { value: V; label: string; title?: string };
  type Props = {
    options: readonly Option[];
    value: V;
    /** What the group is called: the id of the element that says it. */
    labelledby?: string;
    label?: string;
    disabled?: boolean;
    onchange: (value: V) => void;
  };
  let { options, value, labelledby, label, disabled = false, onchange }: Props = $props();

  let group: HTMLElement | undefined = $state();

  function onkeydown(e: KeyboardEvent): void {
    const step =
      e.key === "ArrowRight" || e.key === "ArrowDown"
        ? 1
        : e.key === "ArrowLeft" || e.key === "ArrowUp"
          ? -1
          : 0;
    if (step === 0 || disabled) return;
    e.preventDefault();
    const at = options.findIndex((o) => o.value === value);
    const to = (at + step + options.length) % options.length;
    const next = options[to];
    if (!next) return;
    onchange(next.value);
    void tick().then(() => group?.querySelectorAll<HTMLElement>('[role="radio"]')[to]?.focus());
  }
</script>

<div
  class="segments"
  role="radiogroup"
  aria-labelledby={labelledby}
  aria-label={label}
  bind:this={group}
>
  {#each options as option (option.value)}
    <button
      type="button"
      role="radio"
      class="segment"
      class:is-on={option.value === value}
      aria-checked={option.value === value}
      tabindex={option.value === value ? 0 : -1}
      title={option.title}
      {disabled}
      {onkeydown}
      onclick={() => onchange(option.value)}
    >
      {option.label}
    </button>
  {/each}
</div>

<style>
  .segments {
    display: inline-flex;
    flex-wrap: wrap;
    gap: 2px;
    padding: 2px;
    border: 1px solid var(--border);
    border-radius: var(--radius-control);
    background: var(--surface);
  }
  .segment {
    min-height: 24px;
    padding: 2px 10px;
    border: 0;
    border-radius: 4px;
    background: transparent;
    color: var(--muted);
    font: inherit;
    font-size: var(--fs-ui-sm);
    cursor: pointer;
    transition: background-color var(--dur-fast) var(--ease);
  }
  .segment:hover:not(:disabled):not(.is-on) {
    background: var(--hover);
    color: var(--text);
  }
  .segment:disabled {
    opacity: 0.5;
    cursor: default;
  }
  .segment.is-on {
    background: var(--bg);
    color: var(--text);
    font-weight: 500;
    box-shadow: 0 0 0 1px var(--border);
  }
</style>

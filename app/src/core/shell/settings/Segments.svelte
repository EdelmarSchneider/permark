<script lang="ts" generics="V extends string">
  // A row of exclusive choices (a radio group drawn as segments). Arrows move the choice (and the focus) like any
  // radio group.
  import { tick } from "svelte";

  /** `title`: tooltip; `why`: why the choice is off (tooltip + its accessible description). */
  type Option = { value: V; label: string; title?: string; why?: string };
  type Props = {
    options: readonly Option[];
    value: V;
    /** What the group is called: the id of the element that says it. */
    labelledby?: string;
    label?: string;
    disabled?: boolean;
    /** Choices that are off here (they say why in their `title`; arrows skip them). */
    disabledValues?: readonly V[];
    onchange: (value: V) => void;
  };
  let {
    options,
    value,
    labelledby,
    label,
    disabled = false,
    disabledValues = [],
    onchange,
  }: Props = $props();

  let group: HTMLElement | undefined = $state();
  const uid = $props.id();

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
    let to = at;
    for (let n = 0; n < options.length; n += 1) {
      to = (to + step + options.length) % options.length;
      if (!disabledValues.includes(options[to]?.value as V)) break;
    }
    const next = options[to];
    if (!next || to === at) return;
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
      class:is-off={disabledValues.includes(option.value)}
      aria-checked={option.value === value}
      aria-disabled={disabledValues.includes(option.value) ? "true" : undefined}
      tabindex={option.value === value ? 0 : -1}
      title={option.why ?? option.title}
      aria-describedby={option.why === undefined ? undefined : `${uid}-why-${option.value}`}
      {disabled}
      {onkeydown}
      onclick={() => {
        if (!disabledValues.includes(option.value)) onchange(option.value);
      }}
    >
      {option.label}
    </button>
  {/each}
  {#each options as option (option.value)}
    {#if option.why !== undefined}
      <span class="sr-only" id="{uid}-why-{option.value}">{option.why}</span>
    {/if}
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
  .segment:hover:not(:disabled):not(.is-on):not(.is-off) {
    background: var(--hover);
    color: var(--text);
  }
  .segment:disabled,
  .segment.is-off {
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

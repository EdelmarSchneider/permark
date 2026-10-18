<script lang="ts" module>
  export type SuggestApi = {
    /** A key the editor passed on while the menu is open. Resolves to whether the menu took it. */
    key(key: string): boolean;
  };
</script>

<script lang="ts">
  // Words that continue what was just typed. The caret keeps the focus: the editor hands over the keys.
  import { onMount } from "svelte";
  import type { CompleteItem } from "../editor/complete";
  import type { CompleteState } from "../editor/visual/editor";
  import type { IconName } from "./Icon.svelte";
  import { t } from "../i18n";
  import { registerOverlay } from "../overlays";
  import Icon from "./Icon.svelte";

  type Props = {
    completion: CompleteState;
    onpick(item: CompleteItem): void;
    onclose(): void;
    api?: SuggestApi;
  };
  let { completion, onpick, onclose, api = $bindable() }: Props = $props();

  let index = $state(0);
  let el: HTMLElement | undefined = $state();
  let place = $state<{ left: number; top: number } | null>(null);

  const items = $derived(completion.items);
  $effect(() => {
    void completion.query;
    index = 0;
  });

  $effect(() => {
    const rect = completion.rect;
    void items.length;
    if (!el || !rect) return;
    const h = el.offsetHeight;
    const w = el.offsetWidth;
    const below = rect.bottom + 6;
    const top = below + h > window.innerHeight - 8 ? Math.max(8, rect.top - h - 6) : below;
    place = { left: Math.max(8, Math.min(window.innerWidth - w - 8, rect.left)), top };
  });

  function iconFor(kind: CompleteItem["kind"]): IconName {
    if (kind === "file") return "file";
    if (kind === "heading") return "heading";
    return "pilcrow";
  }

  onMount(() => {
    api = {
      key(key) {
        if (key === "ArrowDown" || key === "ArrowUp") {
          if (items.length > 0) {
            index = (index + (key === "ArrowDown" ? 1 : -1) + items.length) % items.length;
            el?.querySelector(".is-active")?.scrollIntoView({ block: "nearest" });
          }
          return true;
        }
        if (key === "Enter" && items.length > 0) {
          onpick(items[index]);
          return true;
        }
        return false;
      },
    };
    const forget = registerOverlay(onclose);
    return () => {
      forget();
      api = undefined;
    };
  });
</script>

<div
  bind:this={el}
  class="suggest"
  role="listbox"
  aria-label={t("complete.label")}
  style:left="{place?.left ?? 0}px"
  style:top="{place?.top ?? 0}px"
  style:visibility={place ? "visible" : "hidden"}
>
  {#each items as item, i (`${item.kind}:${item.insert}:${i}`)}
    <button
      type="button"
      role="option"
      tabindex="-1"
      aria-selected={i === index}
      class={["suggest__item", i === index && "is-active"]}
      title={item.detail || item.label}
      onmousedown={(e) => e.preventDefault()}
      onmousemove={() => (index = i)}
      onclick={() => onpick(item)}
    >
      <Icon name={iconFor(item.kind)} size="sm" />
      <span class="suggest__label">{item.label}</span>
      {#if item.detail}
        <span class="suggest__detail">{item.detail}</span>
      {/if}
    </button>
  {/each}
</div>

<script lang="ts" module>
  export type SlashApi = {
    /** A key the editor passed on while the menu is open. Resolves to whether the menu took it. */
    key(key: string): boolean;
  };
</script>

<script lang="ts">
  // The slash menu of Visual mode (docs/design/v0.2.md section 3): `/` at the start of an empty block lists the block types; typing
  // filters, Up and Down move, Enter picks, Esc closes. The focus stays in the text: the editor hands the keys over.
  import { onMount } from "svelte";
  import type { SlashState } from "../editor/visual/editor";
  import type { SlashItem } from "../editor/visual/toolbarModel";
  import { SLASH_ITEMS, filterSlash } from "../editor/visual/toolbarModel";
  import { t } from "../i18n";
  import { registerOverlay } from "../overlays";
  import Icon from "./Icon.svelte";

  type Props = {
    slash: SlashState;
    onpick(item: SlashItem): void;
    onclose(): void;
    api?: SlashApi;
  };
  let { slash, onpick, onclose, api = $bindable() }: Props = $props();

  let index = $state(0);
  let el: HTMLElement | undefined = $state();
  let place = $state<{ left: number; top: number } | null>(null);

  const items = $derived(filterSlash(SLASH_ITEMS, slash.query, t));
  // the filter changed: start from the top again
  $effect(() => {
    void slash.query;
    index = 0;
  });

  // below the caret, or above it when there is no room
  $effect(() => {
    const rect = slash.rect;
    void items.length;
    if (!el || !rect) return;
    const h = el.offsetHeight;
    const w = el.offsetWidth;
    const below = rect.bottom + 6;
    const top = below + h > window.innerHeight - 8 ? Math.max(8, rect.top - h - 6) : below;
    place = { left: Math.max(8, Math.min(window.innerWidth - w - 8, rect.left)), top };
  });

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
  class="slash"
  role="listbox"
  aria-label={t("slash.label")}
  style:left="{place?.left ?? 0}px"
  style:top="{place?.top ?? 0}px"
  style:visibility={place ? "visible" : "hidden"}
>
  {#each items as item, i (item.id)}
    <!-- the text keeps the focus: the options are clicked, and reached by the keys the editor hands over -->
    <button
      type="button"
      role="option"
      tabindex="-1"
      aria-selected={i === index}
      class={["slash__item", i === index && "is-active"]}
      onmousedown={(e) => e.preventDefault()}
      onmousemove={() => (index = i)}
      onclick={() => onpick(item)}
    >
      <Icon name={item.icon} size="sm" />
      <span>{t(item.label)}</span>
    </button>
  {:else}
    <div class="slash__empty" role="status">{t("slash.empty")}</div>
  {/each}
</div>

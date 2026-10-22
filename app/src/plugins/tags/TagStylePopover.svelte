<script lang="ts">
  // The look of one tag: a color from the palette and an icon from the small set; "none" takes it away. Applied as it is
  // picked; Escape or a click elsewhere closes it. An official tag whose look the official list sets says so instead of
  // offering pickers that would change nothing; a local | unlisted one offers "Make official in {root}" (asks first).
  import { onMount } from "svelte";
  import type { TagStatus } from "../../api";
  import { t } from "../../core/i18n";
  import PromoteAsk from "../../core/projects/PromoteAsk.svelte";
  import { projects } from "../../core/projects/instance.svelte";
  import { officialEntry } from "../../core/projects/vocabulary";
  import { TAG_ICONS } from "./icons";
  import { COLOR_LABEL, COLORS, ICON_LABEL, ICONS, isColor, isIcon } from "./model";
  import { tags } from "./store.svelte";

  type Props = { tag: string; close: () => void; status?: TagStatus | null };
  let { tag, close, status = null }: Props = $props();

  let root: HTMLElement | undefined = $state();
  const style = $derived(tags.styleOf(tag));
  const color = $derived(isColor(style?.color) ? style.color : null);
  const icon = $derived(isIcon(style?.icon) ? style.icon : null);
  /** The official list sets this tag's look: the app's own look would not show. */
  const fixed = $derived.by(() => {
    if (status !== "official") return false;
    const entry = officialEntry(projects.view, "tag", tag);
    return entry !== null && (entry.color !== null || entry.icon !== null);
  });
  const promote = $derived(status !== "official" && tags.promotable);

  onMount(() => {
    const outside = (event: PointerEvent): void => {
      if (root && event.target instanceof Node && !root.contains(event.target)) close();
    };
    window.addEventListener("pointerdown", outside, true);
    return () => window.removeEventListener("pointerdown", outside, true);
  });

  function onkeydown(event: KeyboardEvent): void {
    if (event.key === "Escape") {
      event.preventDefault();
      event.stopPropagation();
      close();
    }
  }
</script>

<div
  class="pop"
  bind:this={root}
  role="dialog"
  aria-label={t("tags.style.title", { name: tag })}
  tabindex="-1"
  {onkeydown}
>
  {#if fixed}
    <p class="note">{t("tags.officialLook", { root: projects.rootName })}</p>
  {:else}
    <p class="label">{t("tags.style.color")}</p>
    <div class="grid">
      {#each COLORS as name (name)}
        <button
          type="button"
          class="swatch"
          style:--c={`var(--tag-${name})`}
          aria-pressed={color === name}
          title={t(COLOR_LABEL[name])}
          aria-label={t(COLOR_LABEL[name])}
          onclick={() => void tags.setStyle(tag, name, icon)}
        ></button>
      {/each}
      <button
        type="button"
        class="none"
        aria-pressed={color === null}
        onclick={() => void tags.setStyle(tag, null, icon)}
      >
        {t("tags.style.noColor")}
      </button>
    </div>
    <p class="label">{t("tags.style.icon")}</p>
    <div class="grid">
      {#each ICONS as name (name)}
        {@const Icon = TAG_ICONS[name]}
        <button
          type="button"
          class="glyph"
          aria-pressed={icon === name}
          title={t(ICON_LABEL[name])}
          aria-label={t(ICON_LABEL[name])}
          onclick={() => void tags.setStyle(tag, color, name)}
        >
          <Icon size={14} />
        </button>
      {/each}
      <button
        type="button"
        class="none"
        aria-pressed={icon === null}
        onclick={() => void tags.setStyle(tag, color, null)}
      >
        {t("tags.style.noIcon")}
      </button>
    </div>
  {/if}
  {#if promote}
    <div class="official">
      <PromoteAsk kind="tag" name={tag} done={() => root?.focus()} />
    </div>
  {/if}
</div>

<style>
  .pop {
    position: absolute;
    top: 100%;
    left: 0;
    z-index: 30;
    width: 100%;
    min-width: 220px;
    padding: var(--sp-2);
    border: 1px solid var(--border-strong);
    border-radius: var(--radius-card);
    background: var(--surface);
    box-shadow: var(--shadow-pop);
  }
  .note {
    margin: var(--sp-1) 0;
    color: var(--muted);
    font-size: var(--fs-ui-sm);
  }
  .official {
    margin-top: var(--sp-2);
    padding-top: var(--sp-2);
    border-top: 1px solid var(--border);
  }
  .label {
    margin: var(--sp-1) 0;
    color: var(--muted);
    font-size: var(--fs-ui-xs);
  }
  .grid {
    display: flex;
    flex-wrap: wrap;
    gap: var(--sp-1);
  }
  .swatch {
    width: 20px;
    height: 20px;
    border: 2px solid transparent;
    border-radius: 50%;
    background: var(--c);
    cursor: pointer;
  }
  .glyph {
    display: inline-grid;
    place-items: center;
    width: 26px;
    height: 26px;
    border: 1px solid transparent;
    border-radius: var(--radius-control);
    background: transparent;
    color: var(--text);
    cursor: pointer;
  }
  .glyph:hover {
    background: var(--hover);
  }
  .none {
    padding: 0 var(--sp-2);
    border: 1px solid var(--border);
    border-radius: var(--radius-control);
    background: transparent;
    color: var(--muted);
    font: inherit;
    font-size: var(--fs-ui-xs);
    cursor: pointer;
  }
  [aria-pressed="true"] {
    border-color: var(--accent);
    outline: 1px solid var(--accent);
    outline-offset: 1px;
  }
</style>

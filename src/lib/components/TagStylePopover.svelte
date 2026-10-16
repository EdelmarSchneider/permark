<script lang="ts">
  import { onMount, tick } from "svelte";
  import { t } from "../i18n";
  import { tags } from "../stores/tags.svelte";
  import {
    COLOR_LABELS,
    gridMove,
    ICON_LABELS,
    TAG_COLORS,
    TAG_ICONS,
    type TagColor,
    type TagIcon,
  } from "../tagStyle";
  import Icon from "./Icon.svelte";
  import Popover, { type Anchor } from "./Popover.svelte";
  import TagChip from "./TagChip.svelte";

  // Gives one tag a color and an icon. Every pick is saved at once and shows on the preview (and everywhere the tag is
  // drawn); Escape or a click outside closes it.
  type Props = {
    tag: string;
    anchor: Anchor;
    /** `meta.toml` cannot be read: the looks cannot be changed now. */
    readOnly?: boolean;
    onclose: () => void;
  };
  let { tag, anchor, readOnly = false, onclose }: Props = $props();

  /** "None" first, then the palette and the icon set. */
  const COLORS: (TagColor | null)[] = [null, ...TAG_COLORS];
  const ICONS: (TagIcon | null)[] = [null, ...TAG_ICONS];
  const ICON_COLUMNS = 7;

  const look = $derived(tags.styleOf(tag));
  const color = $derived(look?.color ?? null);
  const icon = $derived(look?.icon ?? null);

  let colorGroup: HTMLElement | undefined = $state();
  let iconGroup: HTMLElement | undefined = $state();

  function pickColor(next: TagColor | null): void {
    if (!readOnly) void tags.setStyle(tag, next, icon);
  }
  function pickIcon(next: TagIcon | null): void {
    if (!readOnly) void tags.setStyle(tag, color, next);
  }

  /** Arrow keys move the selection through a group of radio buttons, as they do in any radio group. */
  async function moveIn<T>(
    e: KeyboardEvent,
    group: HTMLElement | undefined,
    options: readonly T[],
    current: T,
    columns: number,
    choose: (value: T) => void,
  ): Promise<void> {
    if (e.ctrlKey || e.metaKey || e.altKey || readOnly) return;
    const next = gridMove(e.key, options.indexOf(current), options.length, columns);
    if (next === null) return;
    e.preventDefault();
    choose(options[next]);
    await tick();
    group?.querySelectorAll<HTMLElement>('[role="radio"]')[next]?.focus();
  }

  onMount(() => {
    // the popover is positioned in an effect; focus once it is in place
    const timer = setTimeout(() => {
      colorGroup?.querySelector<HTMLElement>('[role="radio"][aria-checked="true"]')?.focus();
    }, 0);
    return () => clearTimeout(timer);
  });
</script>

<Popover
  {anchor}
  side="right"
  variant="panel"
  label={t("tags.style.title", { name: tag })}
  {onclose}
>
  <div class="tag-style">
    <div class="tag-style__preview"><TagChip name={tag} /></div>

    {#if readOnly}
      <div class="tag-editor__note">{t("tags.readOnly")}</div>
    {/if}

    <div>
      <div class="tag-style__label" id="tag-style-color">{t("tags.style.color")}</div>
      <!-- the arrow keys are handled on the group, as on any radio group -->
      <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
      <div
        bind:this={colorGroup}
        class="tag-style__colors"
        role="radiogroup"
        aria-labelledby="tag-style-color"
        tabindex="-1"
        onkeydown={(e) => moveIn(e, colorGroup, COLORS, color, COLORS.length, pickColor)}
      >
        {#each COLORS as option (option ?? "none")}
          {@const label = option === null ? t("tags.style.noColor") : t(COLOR_LABELS[option])}
          <button
            type="button"
            role="radio"
            class={["tag-swatch", option === null && "tag-swatch--none"]}
            data-tag-color={option ?? undefined}
            aria-checked={color === option}
            aria-label={label}
            title={label}
            tabindex={color === option ? 0 : -1}
            disabled={readOnly}
            onclick={() => pickColor(option)}
          >
            {#if option !== null && color === option}<Icon name="check" size="xs" />{/if}
          </button>
        {/each}
      </div>
    </div>

    <div>
      <div class="tag-style__label" id="tag-style-icon">{t("tags.style.icon")}</div>
      <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
      <div
        bind:this={iconGroup}
        class="tag-style__icons"
        role="radiogroup"
        aria-labelledby="tag-style-icon"
        tabindex="-1"
        data-tag-color={color ?? undefined}
        onkeydown={(e) => moveIn(e, iconGroup, ICONS, icon, ICON_COLUMNS, pickIcon)}
      >
        {#each ICONS as option (option ?? "none")}
          {@const label = option === null ? t("tags.style.noIcon") : t(ICON_LABELS[option])}
          <button
            type="button"
            role="radio"
            class="tag-icon-btn"
            aria-checked={icon === option}
            aria-label={label}
            title={label}
            tabindex={icon === option ? 0 : -1}
            disabled={readOnly}
            onclick={() => pickIcon(option)}
          >
            {#if option === null}
              <span class="tag-none-glyph"></span>
            {:else}
              <Icon name={option} size="sm" />
            {/if}
          </button>
        {/each}
      </div>
    </div>
  </div>
</Popover>

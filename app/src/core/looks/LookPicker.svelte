<script lang="ts">
  // A color from the palette and an icon from the small set; "none" takes it away. Applied as it is picked (the caller
  // saves). Used by a tag's look (tags plugin) and an official name's look (Settings > Projects inside).
  import { t } from "../i18n";
  import { TAG_ICONS } from "./icons";
  import { COLOR_LABEL, COLORS, ICON_LABEL, ICONS, type TagColor, type TagIconName } from "./model";

  type Props = {
    color: TagColor | null;
    icon: TagIconName | null;
    pick: (color: TagColor | null, icon: TagIconName | null) => void;
    disabled?: boolean;
  };
  let { color, icon, pick, disabled = false }: Props = $props();
</script>

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
      {disabled}
      onclick={() => pick(name, icon)}
    ></button>
  {/each}
  <button
    type="button"
    class="none"
    aria-pressed={color === null}
    {disabled}
    onclick={() => pick(null, icon)}
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
      {disabled}
      onclick={() => pick(color, name)}
    >
      <Icon size={14} />
    </button>
  {/each}
  <button
    type="button"
    class="none"
    aria-pressed={icon === null}
    {disabled}
    onclick={() => pick(color, null)}
  >
    {t("tags.style.noIcon")}
  </button>
</div>

<style>
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
    width: 24px;
    height: 24px;
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
    min-height: 24px;
    padding: 0 var(--sp-2);
    border: 1px solid var(--border);
    border-radius: var(--radius-control);
    background: transparent;
    color: var(--muted);
    font: inherit;
    font-size: var(--fs-ui-xs);
    cursor: pointer;
  }
  button:disabled {
    opacity: 0.5;
    cursor: default;
  }
  [aria-pressed="true"] {
    border-color: var(--accent);
    outline: 1px solid var(--accent);
    outline-offset: 1px;
  }
</style>

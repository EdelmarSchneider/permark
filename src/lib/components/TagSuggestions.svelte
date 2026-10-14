<script lang="ts">
  import { t, tn } from "../i18n";
  import type { Suggestion } from "../tagModel";
  import { highlightSegments } from "../text";

  type Props = {
    id: string;
    rows: Suggestion[];
    active: number;
    label?: string;
    onpick: (row: Suggestion) => void;
    onhover?: (index: number) => void;
  };
  let { id, rows, active, label = t("tags.suggestions"), onpick, onhover }: Props = $props();
</script>

<div {id} class="tag-suggest" role="listbox" aria-label={label}>
  {#each rows as row, i (row.type === "tag" ? row.match.tag.name : `create:${row.name}`)}
    <!-- the input keeps the focus; the list is driven from there (aria-activedescendant) -->
    <!-- svelte-ignore a11y_click_events_have_key_events -->
    <div
      id="{id}-{i}"
      class={["tag-suggest__row", i === active && "is-active"]}
      role="option"
      tabindex="-1"
      aria-selected={i === active}
      onmousedown={(e) => e.preventDefault()}
      onmousemove={() => onhover?.(i)}
      onclick={() => onpick(row)}
    >
      {#if row.type === "tag"}
        <span class="tag-suggest__name">
          {#each highlightSegments(row.match.tag.name, row.match.positions) as seg, s (s)}{#if seg.match}<b
                >{seg.text}</b
              >{:else}{seg.text}{/if}{/each}
        </span>
        <span class="tag-suggest__count">{tn("tags.filesCount", row.match.tag.count)}</span>
      {:else}
        <span class="tag-suggest__name">{t("tags.createTag", { name: row.name })}</span>
      {/if}
    </div>
  {/each}
</div>

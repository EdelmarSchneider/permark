<script lang="ts">
  import { t } from "../i18n";
  import { app } from "../stores/app.svelte";
  import { docs } from "../stores/docs.svelte";
  import { session } from "../stores/session.svelte";
  import { tags } from "../stores/tags.svelte";
  import { ui } from "../stores/ui.svelte";
  import { vault } from "../stores/vault.svelte";
  import Icon from "./Icon.svelte";
  import TagChip from "./TagChip.svelte";

  // the tags are loaded together with the document; one for another path is not shown
  const meta = $derived(tags.meta?.path === docs.path ? tags.meta : null);
  const visible = $derived(meta !== null && (meta.description !== "" || meta.tags.length > 0));
  const canEdit = $derived(meta !== null && meta.writable && !vault.isSimple);
  // Simple mode has no Tags view to show the files in
  const canBrowse = $derived(!vault.isSimple);
</script>

{#if meta && visible}
  <div
    class="meta-strip"
    style:--zoom={session.zoom}
    aria-label={t("tags.popoverTitle")}
    role="group"
  >
    {#if meta.description !== ""}
      <p class="meta-strip__desc">{meta.description}</p>
    {/if}
    {#if meta.tags.length > 0 || canEdit}
      <div class="meta-strip__chips">
        {#each meta.tags as tag (tag.name)}
          <TagChip
            name={tag.name}
            file={tag.file}
            title={t("tags.chipHint", { name: tag.name })}
            onclick={canBrowse ? () => app.showTagsFor(tag.name) : undefined}
          />
        {/each}
        {#if canEdit}
          <button
            type="button"
            class="tag-add"
            data-tag-anchor
            data-popover-trigger
            aria-label={t("tags.add")}
            title={t("tags.addHint")}
            aria-expanded={ui.tagEditor !== null}
            onclick={(e) => ui.openTagEditor(e.currentTarget.getBoundingClientRect())}
          >
            <Icon name="plus" size="xs" />
          </button>
        {/if}
      </div>
    {/if}
  </div>
{/if}

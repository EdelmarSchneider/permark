<script lang="ts">
  import { t } from "../i18n";
  import { tags } from "../stores/tags.svelte";
  import Icon from "./Icon.svelte";

  type Props = {
    name: string;
    /** From the file's front matter: the app cannot change it. */
    file?: boolean;
    /** Shows an x that calls `onremove`. */
    removable?: boolean;
    /** The tag is no tag of any file (a filter that finds nothing). */
    unknown?: boolean;
    title?: string;
    /** Makes the chip a button. */
    onclick?: () => void;
    onremove?: () => void;
    /** Label of the remove button (it names the tag). */
    removeLabel?: string;
  };
  let {
    name,
    file = false,
    removable = false,
    unknown = false,
    title,
    onclick,
    onremove,
    removeLabel,
  }: Props = $props();

  const hint = $derived(file ? t("tags.fromFileHint") : title);
  // the tag's own color and icon, when it has them (a tag with neither looks as it always did)
  const look = $derived(tags.styleOf(name));
</script>

{#snippet content()}
  {#if look?.icon}
    <Icon name={look.icon} size="xs" />
  {:else if file}
    <Icon name="file-text" size="xs" />
  {/if}
  <span class="tag-chip__name">{name}</span>
{/snippet}

<span
  class={[
    "tag-chip",
    file && "tag-chip--file",
    unknown && "tag-chip--unknown",
    look?.color && "tag-chip--colored",
  ]}
  data-tag-color={look?.color ?? undefined}
  title={hint}
>
  {#if onclick}
    <button type="button" class="tag-chip__body" {onclick}>{@render content()}</button>
  {:else}
    <span class="tag-chip__body">{@render content()}</span>
  {/if}
  {#if removable}
    <button
      type="button"
      class="tag-chip__remove"
      aria-label={removeLabel ?? t("tags.removeTag", { name })}
      title={removeLabel ?? t("tags.removeTag", { name })}
      onclick={onremove}
    >
      <Icon name="x" size="xs" />
    </button>
  {/if}
</span>

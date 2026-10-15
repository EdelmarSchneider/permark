<script lang="ts">
  import { t } from "../i18n";

  type Props = {
    /** What the dot says (to a screen reader and as a tooltip): "Unread", or "3 unread" on a folder. */
    label?: string;
  };
  let { label }: Props = $props();
  const text = $derived(label ?? t("unread.dot"));
</script>

<span class="unread-dot" role="img" aria-label={text} title={text}></span>

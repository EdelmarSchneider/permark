<script lang="ts">
  // A document with no page to show: a file that is not text, too large to render, or with no viewer on. Says why in
  // one plain sentence and, when it can, offers the system's program.
  import type { Notice } from "../../api";
  import { t } from "../i18n";
  import { documents } from "./document.svelte";

  type Props = { notice: Notice; name: string; path: string; canOpenSystem: boolean };
  let { notice, name, path, canOpenSystem }: Props = $props();

  const title = $derived.by(() => {
    switch (notice.notice) {
      case "binary":
        return t("doc.binary.title");
      case "tooLarge":
        return t("doc.tooLarge.title");
      case "noViewer":
        return t("doc.noViewer.title");
      default:
        return notice satisfies never;
    }
  });

  const text = $derived.by(() => {
    switch (notice.notice) {
      case "binary":
        return t("doc.binary.text", { name });
      case "tooLarge":
        return t("doc.tooLarge.text", { name, limit: Math.round(notice.limit / (1024 * 1024)) });
      case "noViewer":
        return t("doc.noViewer.text", { name });
      default:
        return notice satisfies never;
    }
  });
</script>

<div class="notice" role="status">
  <p class="title">{title}</p>
  <p class="text">{text}</p>
  {#if canOpenSystem}
    <button
      type="button"
      class="btn"
      onclick={() => void documents.openExternal(path, "defaultApp")}
    >
      {t("menu.openDefaultApp")}
    </button>
  {/if}
</div>

<style>
  .notice {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: var(--sp-2);
    padding: var(--sp-8) var(--sp-5);
    text-align: center;
  }
  p {
    margin: 0;
    max-width: var(--measure);
  }
  .title {
    font: 600 var(--fs-reading) / var(--lh-reading) var(--font-reading);
  }
  .text {
    color: var(--muted);
    overflow-wrap: anywhere;
  }
</style>

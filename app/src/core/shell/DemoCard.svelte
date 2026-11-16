<script lang="ts">
  // The Welcome screen's offer of the demo pages: one card ("Explore the demo pages" | "Open the demo pages") with a
  // close button; nothing at all once used or dismissed, or when this copy has no pages to offer.
  import { onMount } from "svelte";
  import Sparkles from "@lucide/svelte/icons/sparkles";
  import X from "@lucide/svelte/icons/x";
  import { t } from "../i18n";
  import type { WindowStore } from "../window.svelte";
  import { demoOffer } from "./demoOffer.svelte";

  type Props = { store: WindowStore; disabled: boolean };
  let { store, disabled }: Props = $props();

  onMount(() => {
    void demoOffer.load();
  });

  const card = $derived(demoOffer.card);
</script>

{#if card.status !== "hidden"}
  <section class="demo" aria-labelledby="demo-title">
    <button
      type="button"
      class="demo-use"
      disabled={disabled || card.status === "working"}
      aria-busy={card.status === "working"}
      onclick={() => void demoOffer.use(store)}
    >
      <span class="icon" aria-hidden="true"><Sparkles size={18} /></span>
      <span class="text">
        <span id="demo-title" class="title">
          {card.offer === "install" ? t("welcome.demo.explore") : t("welcome.demo.open")}
        </span>
        <span class="hint">{t("welcome.demo.hint")}</span>
      </span>
    </button>
    <button
      type="button"
      class="icon-btn dismiss"
      aria-label={t("common.dismiss")}
      title={t("common.dismiss")}
      disabled={card.status === "working"}
      onclick={() => void demoOffer.dismiss()}
    >
      <X size={14} aria-hidden="true" />
    </button>
    {#if card.status === "offered" && card.problem !== null}
      <p class="problem" role="alert">{card.problem}</p>
    {/if}
  </section>
{/if}

<style>
  .demo {
    position: relative;
    width: min(460px, 100%);
    margin-top: var(--sp-5);
  }
  .demo-use {
    display: flex;
    align-items: center;
    gap: var(--sp-3);
    width: 100%;
    /* room for the close button in the corner */
    padding: 10px 40px 10px var(--sp-3);
    border: 1px solid var(--border);
    border-radius: var(--radius-card);
    background: var(--surface);
    color: var(--text);
    font: inherit;
    text-align: start;
    cursor: pointer;
    transition:
      background-color var(--dur-fast) var(--ease),
      border-color var(--dur-fast) var(--ease);
  }
  .demo-use:hover:not(:disabled) {
    border-color: var(--border-strong);
    background: var(--accent-soft);
  }
  .demo-use:disabled {
    cursor: default;
    opacity: 0.6;
  }
  .icon {
    display: grid;
    flex: none;
    place-items: center;
    width: 34px;
    height: 34px;
    border-radius: var(--radius-control);
    background: var(--accent-soft);
    color: var(--accent-text);
  }
  .text {
    display: flex;
    flex-direction: column;
    gap: 2px;
    min-width: 0;
  }
  .title {
    font-size: 13px;
    font-weight: 600;
  }
  .hint {
    color: var(--muted);
    font-size: var(--fs-ui-sm);
  }
  .dismiss {
    position: absolute;
    top: 6px;
    right: 6px;
  }
  .problem {
    margin: 6px 0 0;
    color: var(--git-deleted-text);
    font-size: var(--fs-ui-sm);
    text-align: center;
  }
</style>

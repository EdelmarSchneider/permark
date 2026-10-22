<script lang="ts">
  // The once-only strip under the Explorer list of a project holding agent files: what PerMark can share, "Set up…"
  // (opens the settings section; turns nothing on) and "No thanks". Never takes focus by itself; after an answer the
  // strip goes and focus goes back to the list.
  import type { ExplorerNoteProps } from "../../core/plugins/registry.svelte";
  import { t } from "../../core/i18n";
  import { offer } from "./offer.svelte";

  let { focusList }: ExplorerNoteProps = $props();

  const name = $derived(offer.files[0] ?? "");
</script>

<div class="offer" role="region" aria-label={t("agent.offer.label")}>
  <span>{t("agent.offer.text", { name })}</span>
  <button
    type="button"
    class="btn btn--sm"
    onclick={() => {
      // the page gives focus back where it was when it closes: the list, not this strip (it goes)
      focusList();
      void offer.setUp();
    }}
  >
    {t("agent.setUp")}
  </button>
  <button
    type="button"
    class="link"
    onclick={() => {
      void offer.decline();
      focusList();
    }}
  >
    {t("agent.offer.decline")}
  </button>
</div>

<style>
  /* the code offer's look (ExplorerPanel `.code-offer`): under the list, above the counts line, muted */
  .offer {
    display: flex;
    align-items: center;
    flex-wrap: wrap;
    gap: var(--sp-2);
    padding: 6px var(--sp-3);
    border-top: 1px solid var(--border);
    color: var(--muted);
    font-size: var(--fs-ui-xs);
  }
  .link {
    padding: 0;
    border: 0;
    background: transparent;
    color: var(--accent-text);
    font: inherit;
    cursor: pointer;
  }
</style>

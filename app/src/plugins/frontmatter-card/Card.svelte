<script lang="ts">
  // The properties card: front matter as rows of keys and values. Lists are chips, dates are written for the
  // reader. Text the plugin could not read as properties shows as it is.
  import { language, t } from "../../core/i18n";
  import { readerDate } from "../../core/time";
  import type { FrontMatterProps } from "../../core/plugins/props";

  let { fields, raw, syntax }: FrontMatterProps = $props();

  /** A plain date in the reader's language; anything else as written. */
  const show = (text: string): string => readerDate(text, language.locale);
</script>

<section class="card" aria-label={t("doc.frontmatter")}>
  {#if fields.length === 0}
    <pre class="raw" data-syntax={syntax}>{raw.trim()}</pre>
  {:else}
    <dl class="grid">
      {#each fields as field (field.key)}
        <div class="field">
          <dt>{field.key}</dt>
          {#if field.value.type === "list"}
            <dd class="chips">
              {#each field.value.items as item, at (at)}<span class="chip">{item}</span>{/each}
            </dd>
          {:else}
            <dd>{show(field.value.text)}</dd>
          {/if}
        </div>
      {/each}
    </dl>
  {/if}
</section>

<style>
  .card {
    margin: 0 0 2em;
    padding: 12px 16px;
    border: 1px solid var(--border);
    border-radius: var(--radius-card);
    background: var(--surface);
    font: 13px/1.4 var(--font-ui);
  }
  .grid {
    display: grid;
    grid-template-columns: minmax(0, 1fr);
    gap: 10px;
    margin: 0;
  }
  .field {
    display: grid;
    grid-template-columns: minmax(110px, 26%) minmax(0, 1fr);
    gap: var(--sp-3);
    align-items: baseline;
    min-width: 0;
  }
  dt {
    min-width: 0;
    overflow-wrap: anywhere;
    color: var(--muted);
    font-family: var(--font-code);
    font-size: 0.95em;
  }
  dd {
    margin: 0;
    min-width: 0;
    overflow-wrap: anywhere;
  }
  .chips {
    display: flex;
    gap: 4px;
    flex-wrap: wrap;
  }
  .chip {
    padding: 0 7px;
    border-radius: 10px;
    background: var(--surface-2);
    color: var(--muted);
    font-size: 12px;
  }
  .raw {
    margin: 0;
    white-space: pre-wrap;
    font: 12.5px/1.5 var(--font-code);
    color: var(--muted);
  }
  @media (max-width: 560px) {
    .field {
      grid-template-columns: minmax(0, 1fr);
    }
  }
</style>

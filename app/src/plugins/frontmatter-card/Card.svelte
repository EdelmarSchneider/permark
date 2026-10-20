<script lang="ts">
  // The properties card: front matter as a grid of keys and values. Lists are chips, dates are written for the
  // reader. Text the plugin could not read as properties shows as it is.
  import { language, t } from "../../core/i18n";
  import type { FrontMatterProps } from "../../core/plugins/props";

  let { fields, raw, syntax }: FrontMatterProps = $props();

  const DATE = /^\d{4}-\d{2}-\d{2}$/;

  /** A plain date (`2026-10-19`) in the reader's language; anything else as written. */
  function show(text: string): string {
    if (!DATE.test(text)) return text;
    const date = new Date(`${text}T00:00:00`);
    return Number.isNaN(date.getTime())
      ? text
      : date.toLocaleDateString(language.locale, { dateStyle: "long" });
  }
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
    grid-template-columns: repeat(auto-fit, minmax(140px, 1fr));
    gap: 10px 24px;
    margin: 0;
  }
  .field {
    min-width: 0;
  }
  dt {
    font-size: 10.5px;
    font-weight: 600;
    letter-spacing: 0.06em;
    text-transform: uppercase;
    color: var(--faint);
  }
  dd {
    margin: 2px 0 0;
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
</style>

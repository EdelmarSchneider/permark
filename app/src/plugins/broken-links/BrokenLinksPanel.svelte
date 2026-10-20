<script lang="ts">
  // The Broken links tab: the Markdown links that lead nowhere, by document. A row shows the line and the destination as
  // it is written; a click opens the document at that line.
  import { documents } from "../../core/doc/document.svelte";
  import { t, tn } from "../../core/i18n";
  import { brokenLinks } from "./store.svelte";

  const report = $derived(brokenLinks.report);

  function split(path: string): { name: string; folder: string } {
    const at = path.lastIndexOf("/");
    return at < 0
      ? { name: path, folder: "" }
      : { name: path.slice(at + 1), folder: path.slice(0, at) };
  }
</script>

<section class="broken" aria-label={t("brokenLinks.title")}>
  <header>
    <h2>{t("brokenLinks.title")}</h2>
    {#if report && report.total > 0}<span class="count">{report.total}</span>{/if}
  </header>

  <div class="body">
    {#if report === null || !report.ready}
      <p class="note" role="status">{t("brokenLinks.checking")}</p>
    {:else if report.total === 0}
      <p class="note strong">{t("brokenLinks.none")}</p>
    {:else}
      {#each report.docs as doc (doc.path)}
        {@const where = split(doc.path)}
        <h3 title={doc.path}>
          <button type="button" class="doc" onclick={() => documents.navigate(doc.path, null)}>
            <span class="name">{where.name}</span>
            {#if where.folder !== ""}<span class="folder">{where.folder}</span>{/if}
          </button>
          <span class="n">{doc.links.length}</span>
        </h3>
        <ul>
          {#each doc.links as link (`${link.line}:${link.written}`)}
            <li>
              <button
                type="button"
                class="link"
                title={t("brokenLinks.open", { path: doc.path, line: link.line })}
                onclick={() => documents.navigate(doc.path, null, link.line)}
              >
                <span class="line">{link.line}</span>
                <span class="written">{link.written}</span>
              </button>
            </li>
          {/each}
        </ul>
      {/each}
      {#if report.truncated}
        <p class="note">{t("brokenLinks.truncated", { n: report.total })}</p>
      {/if}
    {/if}
    {#if report && report.skipped > 0}
      <p class="note faint">{tn("brokenLinks.skipped", report.skipped)}</p>
    {/if}
  </div>
</section>

<style>
  .broken {
    display: flex;
    flex-direction: column;
    flex: 1;
    min-height: 0;
  }
  header {
    display: flex;
    align-items: center;
    gap: var(--sp-2);
    padding: var(--sp-2) var(--sp-3);
    border-bottom: 1px solid var(--border);
  }
  h2 {
    margin: 0;
    font-size: var(--fs-ui);
    font-weight: 600;
  }
  .count {
    padding: 0 6px;
    border-radius: 999px;
    background: color-mix(in srgb, var(--git-deleted) 16%, transparent);
    color: var(--git-deleted-text);
    font-size: var(--fs-ui-xs);
    font-weight: 600;
  }
  .body {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    padding-bottom: var(--sp-3);
  }
  .note {
    margin: var(--sp-2) var(--sp-3);
    color: var(--muted);
    font-size: var(--fs-ui-sm);
  }
  .strong {
    color: var(--text);
    font-weight: 600;
  }
  .faint {
    color: var(--faint);
  }
  h3 {
    display: flex;
    align-items: baseline;
    gap: var(--sp-2);
    margin: 0;
    padding: var(--sp-2) var(--sp-3) var(--sp-1);
    background: var(--surface-2);
    font-size: var(--fs-ui-sm);
    font-weight: 600;
  }
  .doc {
    display: flex;
    align-items: baseline;
    gap: var(--sp-2);
    min-width: 0;
    padding: 0;
    border: 0;
    background: transparent;
    color: inherit;
    font: inherit;
    text-align: left;
    cursor: pointer;
  }
  .doc:hover .name {
    text-decoration: underline;
  }
  .folder {
    min-width: 0;
    overflow: hidden;
    color: var(--faint);
    font-size: var(--fs-ui-xs);
    font-weight: 400;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .n {
    margin-left: auto;
    color: var(--muted);
    font-variant-numeric: tabular-nums;
  }
  ul {
    margin: 0;
    padding: 0;
    list-style: none;
  }
  .link {
    display: flex;
    gap: var(--sp-2);
    width: 100%;
    padding: 3px var(--sp-3);
    border: 0;
    background: transparent;
    color: var(--text);
    font: inherit;
    font-size: var(--fs-ui-sm);
    text-align: left;
    cursor: pointer;
  }
  .link:hover {
    background: var(--hover);
  }
  .line {
    flex: none;
    min-width: 2.5em;
    color: var(--faint);
    font-variant-numeric: tabular-nums;
    text-align: right;
  }
  .written {
    min-width: 0;
    font-family: var(--font-code);
    font-size: var(--fs-ui-xs);
    overflow-wrap: anywhere;
  }
</style>

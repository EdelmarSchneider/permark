<script lang="ts">
  // The Broken links tab: the Markdown links that lead nowhere, by document. A row shows the line and the destination as
  // it is written; a click opens the document at that line. One same-root file offers Repair; several offer a pick
  // and nothing is written until one is chosen; none offers no control. The count is the opened folder's; when a
  // linked worktree has broken links too, each root gets its own section so none is mistaken for the other.
  import { documents } from "../../core/doc/document.svelte";
  import { t, tn, type MessageKey } from "../../core/i18n";
  import { brokenLinks } from "./store.svelte";

  import type { BrokenDoc, BrokenLink, RepairRefusal } from "../../api";

  /** The prefix of a linked worktree's paths (`@wt/<name>/…`). */
  const WORKTREE = "@wt/";

  const report = $derived(brokenLinks.report);
  /** Sections only when a linked worktree has broken links: a folder without one looks as before. */
  const sectioned = $derived(
    report?.roots.some((root) => root.prefix !== "" && root.total > 0) ?? false,
  );
  const sections = $derived.by((): { prefix: string; total: number; docs: BrokenDoc[] }[] => {
    if (report === null) return [];
    if (!sectioned) return [{ prefix: "", total: report.current, docs: report.docs }];
    return report.roots
      .filter((root) => root.prefix === "" || root.total > 0)
      .map((root) => ({
        ...root,
        docs: report.docs.filter((d) => rootOf(d.path) === root.prefix),
      }));
  });

  function rootOf(path: string): string {
    return path.startsWith(WORKTREE) ? path.split("/").slice(0, 2).join("/") : "";
  }

  function rootTitle(prefix: string): string {
    return prefix === ""
      ? t("brokenLinks.thisFolder")
      : t("brokenLinks.worktree", { name: prefix.slice(WORKTREE.length) });
  }

  /** The name and folder of a document, inside its root (the section says which). */
  function split(path: string, prefix: string): { name: string; folder: string } {
    const own = prefix === "" ? path : path.slice(prefix.length + 1);
    const at = own.lastIndexOf("/");
    return at < 0
      ? { name: own, folder: "" }
      : { name: own.slice(at + 1), folder: own.slice(0, at) };
  }

  let refused = $state.raw<ReadonlyMap<string, RepairRefusal>>(new Map());

  function rowId(path: string, link: BrokenLink): string {
    return `${path}\0${link.line}\0${link.href}`;
  }

  function refusalKey(reason: RepairRefusal): MessageKey {
    switch (reason) {
      case "unsaved":
        return "brokenLinks.refused.unsaved";
      case "unreadable":
        return "brokenLinks.refused.unreadable";
      case "notUtf8":
        return "brokenLinks.refused.notUtf8";
      case "missing":
        return "brokenLinks.refused.missing";
      case "mismatch":
        return "brokenLinks.refused.mismatch";
      case "notCandidate":
        return "brokenLinks.refused.notCandidate";
      case "noReplacement":
        return "brokenLinks.refused.noReplacement";
      case "changed":
        return "brokenLinks.refused.changed";
      case "writeFailed":
        return "brokenLinks.refused.writeFailed";
      default: {
        const exhaustive = reason satisfies never;
        return exhaustive;
      }
    }
  }

  async function repair(path: string, link: BrokenLink, target: string): Promise<void> {
    const id = rowId(path, link);
    const result = await brokenLinks.repair({ path, line: link.line, href: link.href, target });
    if (result === null) return;
    const next = new Map(refused);
    switch (result.outcome) {
      case "wrote":
        next.delete(id);
        break;
      case "refused":
        next.set(id, result.reason);
        break;
      default: {
        const exhaustive = result satisfies never;
        return exhaustive;
      }
    }
    refused = next;
  }
</script>

<section class="broken" aria-label={t("brokenLinks.title")}>
  <header>
    <h2>{t("brokenLinks.title")}</h2>
    {#if report && report.ready && report.current > 0}<span class="count">{report.current}</span
      >{/if}
  </header>

  <div class="body">
    {#if report === null || !report.ready}
      <p class="note" role="status">{t("brokenLinks.checking")}</p>
    {:else if report.total === 0}
      <p class="note strong">{t("brokenLinks.none")}</p>
    {:else}
      {#each sections as section (section.prefix)}
        {#if sectioned}
          {@const title = rootTitle(section.prefix)}
          <h3 class="root">
            <span class="root-name" {title}>{title}</span>
            <span class="n" aria-hidden="true">{section.total}</span>
            <span class="sr-only">{tn("brokenLinks.rootCount", section.total)}</span>
          </h3>
          {#if section.total === 0}
            <p class="note">{t("brokenLinks.noneHere")}</p>
          {:else if section.docs.length === 0}
            <p class="note">{t("brokenLinks.notListed")}</p>
          {/if}
        {/if}
        {#each section.docs as doc (doc.path)}
          {@const where = split(doc.path, section.prefix)}
          <svelte:element this={sectioned ? "h4" : "h3"} class="doc-head" title={doc.path}>
            <button type="button" class="doc" onclick={() => documents.navigate(doc.path, null)}>
              <span class="name">{where.name}</span>
              {#if where.folder !== ""}<span class="folder">{where.folder}</span>{/if}
            </button>
            <span class="n">{doc.links.length}</span>
          </svelte:element>
          <ul>
            {#each doc.links as link (`${link.line}:${link.href}`)}
              {@const only = link.candidates.length === 1 ? link.candidates[0] : undefined}
              {@const reason = refused.get(rowId(doc.path, link))}
              <li>
                <div class="row">
                  <button
                    type="button"
                    class="link"
                    title={t("brokenLinks.open", { path: doc.path, line: link.line })}
                    onclick={() => documents.navigate(doc.path, null, link.line)}
                  >
                    <span class="line">{link.line}</span>
                    <span class="written">{link.written}</span>
                  </button>
                  {#if only}
                    <button
                      type="button"
                      class="repair"
                      onclick={() => {
                        if (only !== undefined) void repair(doc.path, link, only);
                      }}
                    >
                      {t("brokenLinks.repair")}
                    </button>
                  {:else if link.candidates.length > 1}
                    <select
                      class="pick"
                      aria-label={t("brokenLinks.pick")}
                      onchange={(event) => {
                        const chosen = event.currentTarget.value;
                        event.currentTarget.value = "";
                        if (chosen !== "") void repair(doc.path, link, chosen);
                      }}
                    >
                      <option value="">{t("brokenLinks.pick")}</option>
                      {#each link.candidates as target (target)}
                        <option value={target}>{target}</option>
                      {/each}
                    </select>
                  {/if}
                </div>
                {#if reason}<p class="refusal">{t(refusalKey(reason))}</p>{/if}
              </li>
            {/each}
          </ul>
        {/each}
      {/each}
      {#if report.truncated}
        <!-- the header counts the opened folder; the list (and this note) may also hold the worktrees' -->
        <p class="note">
          {t(report.total > report.current ? "brokenLinks.truncatedAll" : "brokenLinks.truncated", {
            n: report.total,
          })}
        </p>
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
  .root {
    display: flex;
    align-items: baseline;
    gap: var(--sp-2);
    margin: var(--sp-2) 0 0;
    padding: var(--sp-1) var(--sp-3);
    border-bottom: 1px solid var(--border);
    color: var(--muted);
    font-size: var(--fs-ui-xs);
    font-weight: 600;
  }
  .root-name {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .doc-head {
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
  .row {
    display: flex;
    align-items: center;
  }
  .link {
    display: flex;
    flex: 1;
    gap: var(--sp-2);
    min-width: 0;
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
  .repair {
    flex: none;
    margin: 2px var(--sp-2) 2px 0;
    padding: 0 6px;
    border: 1px solid var(--border);
    border-radius: var(--radius-control);
    background: transparent;
    color: var(--text);
    font: inherit;
    font-size: var(--fs-ui-xs);
    white-space: nowrap;
    cursor: pointer;
  }
  .repair:hover {
    background: var(--hover);
  }
  .pick {
    flex: none;
    max-width: 50%;
    margin: 2px var(--sp-2) 2px 0;
    color: var(--text);
    font: inherit;
    font-size: var(--fs-ui-xs);
  }
  .refusal {
    margin: 0 var(--sp-3) var(--sp-1);
    color: var(--git-deleted-text);
    font-size: var(--fs-ui-xs);
  }
</style>

<script lang="ts">
  // A ```` ```toon ```` block of a page drawn as cards and tables (read by the backend, as a file would be). A block the
  // pinned spec cannot read stays a code block with the reason. Copy copies the TOON as written; "Copy as JSON" the
  // data. Read only here: in Edit the block is kept as written, Source changes it.
  import Check from "@lucide/svelte/icons/check";
  import Copy from "@lucide/svelte/icons/copy";
  import { api, type Body } from "../../api";
  import ConfigSheet from "../../core/config/ConfigSheet.svelte";
  import CodeActions from "../../core/doc/CodeActions.svelte";
  import { t } from "../../core/i18n";
  import type { EmbedProps } from "../../core/plugins/props";
  import CopyJson from "./CopyJson.svelte";

  let { source }: EmbedProps = $props();

  let body = $state.raw<Body | null>(null);
  let copied = $state(false);

  // the sheet of the block: the name only picks the format, nothing is read from disk
  $effect(() => {
    const text = source;
    let stale = false;
    void api.configSheet("embed.toon", text).then((result) => {
      if (stale) return;
      body = result.ok ? result.value : { kind: "text", text, language: "toon" };
    });
    return () => {
      stale = true;
    };
  });

  $effect(() => {
    if (!copied) return;
    const timer = setTimeout(() => (copied = false), 1500);
    return () => clearTimeout(timer);
  });

  async function copy(): Promise<void> {
    try {
      await navigator.clipboard.writeText(source);
      copied = true;
    } catch {
      // the system refused the clipboard
    }
  }

  const problem = $derived(body?.kind === "text" ? (body.problem ?? null) : null);
</script>

<div class="toon-block" data-state={body === null ? "pending" : body.kind}>
  {#if body?.kind === "sheet"}
    <ConfigSheet sheet={body.sheet} editable={false} source={null} />
  {:else}
    <pre><code>{source}</code></pre>
    {#if body !== null}
      <p class="reason" role="note">
        {t("toon.notValid")}{#if problem}
          ·
          {problem.line > 0 ? t("note.problemAt", { line: problem.line }) : ""}
          <code>{problem.detail}</code>{/if}
      </p>
    {/if}
  {/if}
  <div class="corner">
    <span class="lang">TOON</span>
    <CodeActions {source} language="toon" />
    {#if body?.kind === "sheet"}<CopyJson text={() => Promise.resolve(source)} compact />{/if}
    <button
      type="button"
      class="icon-btn icon-btn--sm copy"
      class:is-done={copied}
      aria-label={t("doc.copyCode")}
      title={t("doc.copyCode")}
      onclick={() => void copy()}
    >
      {#if copied}<Check size={14} />{:else}<Copy size={14} />{/if}
    </button>
  </div>
</div>

<style>
  .toon-block {
    position: relative;
    margin: 0;
    font-family: var(--font-ui);
  }
  .toon-block[data-state="sheet"] {
    border: 1px solid var(--border);
    border-radius: var(--radius-card);
    background: var(--bg);
  }
  .toon-block :global(.sheet) {
    width: 100%;
    padding: 34px var(--sp-3) var(--sp-3);
  }
  pre {
    padding-top: 30px;
  }
  .reason {
    margin: 0;
    padding: 0 var(--sp-3) var(--sp-3);
    color: var(--git-deleted-text);
    font-size: var(--fs-ui-sm);
  }
  .reason code {
    font-family: var(--font-code);
    overflow-wrap: anywhere;
  }
  .corner {
    position: absolute;
    top: 4px;
    right: 6px;
    display: flex;
    align-items: center;
    gap: 2px;
  }
  .lang {
    margin-right: 6px;
    padding: 0 4px;
    color: var(--muted);
    font: 11px / 1.3 var(--font-code);
    user-select: none;
  }
</style>

<script lang="ts">
  // "Initialize PerMark in this folder?": says what is created, then asks. The keyboard starts on the safe button (a
  // stray Enter writes nothing); Escape | it | backdrop = nothing is written. New project on a folder with files in it
  // (`fresh`): says why it asks in plain words, and the safe button goes back to choosing a folder.
  import { t } from "../i18n";

  type Props = { name: string; path: string; fresh?: boolean; answer: (yes: boolean) => void };
  let { name, path, fresh = false, answer }: Props = $props();

  let dialog: HTMLElement | undefined = $state();
  let cancel: HTMLButtonElement | undefined = $state();

  function onkeydown(e: KeyboardEvent): void {
    if (e.key === "Escape") {
      e.preventDefault();
      answer(false);
      return;
    }
    if (e.key !== "Tab" || !dialog) return;
    const focusable = Array.from(dialog.querySelectorAll<HTMLElement>("button:not(:disabled)"));
    const first = focusable[0];
    const last = focusable[focusable.length - 1];
    if (e.shiftKey && document.activeElement === first) {
      e.preventDefault();
      last?.focus();
    } else if (!e.shiftKey && document.activeElement === last) {
      e.preventDefault();
      first?.focus();
    }
  }

  $effect(() => {
    const before = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    cancel?.focus();
    return () => before?.focus();
  });
</script>

<!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
<div
  class="backdrop"
  onclick={(e) => {
    if (e.target === e.currentTarget) answer(false);
  }}
>
  <div
    bind:this={dialog}
    class="dialog"
    role="dialog"
    aria-modal="true"
    aria-labelledby="init-title"
    aria-describedby="init-body"
    tabindex="-1"
    {onkeydown}
  >
    <h2 id="init-title">
      {fresh ? t("project.newTitle", { name }) : t("project.initTitle", { name })}
    </h2>
    <p class="where" title={path}>{path}</p>
    <div id="init-body">
      {#if fresh}
        <p>{t("project.newWhy")}</p>
        <p class="note">{t("project.newNote")}</p>
      {:else}
        <p>{t("project.initIntro")}</p>
        <ul>
          <li><code>.permark/config.toml</code><span>{t("project.initConfig")}</span></li>
          <li><code>.permark/.gitignore</code><span>{t("project.initIgnore")}</span></li>
          <li><code>.permark/local</code><span>{t("project.initLocal")}</span></li>
        </ul>
        <p class="note">{t("project.initNote")}</p>
      {/if}
    </div>
    <footer>
      <button bind:this={cancel} type="button" class="btn" onclick={() => answer(false)}>
        {fresh ? t("project.newBack") : t("common.cancel")}
      </button>
      <button type="button" class="btn btn--primary" onclick={() => answer(true)}>
        {fresh ? t("project.newConfirm") : t("project.initConfirm")}
      </button>
    </footer>
  </div>
</div>

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    z-index: 50;
    display: grid;
    place-items: center;
    padding: var(--sp-5);
    background: var(--backdrop);
    animation: pm-fade var(--dur) var(--ease);
  }
  .dialog {
    width: min(520px, 100%);
    max-height: 100%;
    overflow-y: auto;
    padding: var(--sp-5);
    border: 1px solid var(--border);
    border-radius: var(--radius-card);
    background: var(--bg);
    box-shadow: var(--shadow-pop);
    outline: none;
  }
  h2 {
    margin: 0 0 var(--sp-1);
    font-size: 15px;
    font-weight: 600;
  }
  .where {
    margin: 0 0 var(--sp-4);
    overflow: hidden;
    color: var(--faint);
    font-size: var(--fs-ui-xs);
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  p {
    margin: 0 0 var(--sp-3);
    color: var(--muted);
  }
  ul {
    display: grid;
    gap: var(--sp-2);
    margin: 0 0 var(--sp-3);
    padding: 0;
    list-style: none;
  }
  li {
    display: grid;
    gap: 2px;
    padding: var(--sp-2) var(--sp-3);
    border: 1px solid var(--border);
    border-radius: var(--radius-control);
    background: var(--surface);
    font-size: var(--fs-ui-sm);
  }
  li span {
    color: var(--muted);
  }
  code {
    font-family: var(--font-code);
    font-size: var(--fs-ui-sm);
    color: var(--text);
  }
  .note {
    margin-bottom: var(--sp-5);
    font-size: var(--fs-ui-sm);
  }
  footer {
    display: flex;
    flex-wrap: wrap;
    justify-content: flex-end;
    gap: var(--sp-2);
  }
</style>

<script lang="ts">
  // "Save changes before closing?" One question for the whole window, listing every tab with unsaved edits (tab name +
  // document): Save all | Discard | Cancel. Cancel | Escape | backdrop = nothing changes.
  import type { ClosingTab } from "../../api";
  import { t, tn } from "../i18n";

  type Props = {
    tabs: readonly ClosingTab[];
    answer: (choice: "saveAll" | "discard" | "cancel") => void;
  };
  let { tabs, answer }: Props = $props();

  let dialog: HTMLElement | undefined = $state();
  let save: HTMLButtonElement | undefined = $state();

  /** The file name of a document path (`/` or `\` separated). */
  function fileName(path: string): string {
    return (
      path
        .split(/[\\/]/)
        .filter((p) => p !== "")
        .at(-1) ?? path
    );
  }

  function onkeydown(e: KeyboardEvent): void {
    if (e.key === "Escape") {
      e.preventDefault();
      answer("cancel");
      return;
    }
    if (e.key !== "Tab" || !dialog) return;
    // focus stays inside the dialog
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

  // focus the default answer once shown; give focus back when closed
  $effect(() => {
    const before = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    save?.focus();
    return () => before?.focus();
  });
</script>

<!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
<div
  class="backdrop"
  onclick={(e) => {
    if (e.target === e.currentTarget) answer("cancel");
  }}
>
  <div
    bind:this={dialog}
    class="dialog"
    role="alertdialog"
    aria-modal="true"
    aria-labelledby="close-window-title"
    aria-describedby="close-window-body"
    tabindex="-1"
    data-close-window
    {onkeydown}
  >
    <h2 id="close-window-title">{t("closeWindow.title")}</h2>
    <p id="close-window-body">{tn("closeWindow.body", tabs.length)}</p>
    <ul class="tabs">
      {#each tabs as tab (tab.tab)}
        <li>
          <span class="name">{tab.title}</span>
          {#if tab.document !== "" && fileName(tab.document) !== tab.title}
            <span class="doc">{fileName(tab.document)}</span>
          {/if}
        </li>
      {/each}
    </ul>
    <footer>
      <button type="button" class="btn" onclick={() => answer("cancel")}>
        {t("common.cancel")}
      </button>
      <button type="button" class="btn" onclick={() => answer("discard")}>
        {t("closeWindow.discard")}
      </button>
      <button
        bind:this={save}
        type="button"
        class="btn btn--primary"
        onclick={() => answer("saveAll")}
      >
        {t("closeWindow.saveAll")}
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
    width: min(460px, 100%);
    padding: var(--sp-5);
    border: 1px solid var(--border);
    border-radius: var(--radius-card);
    background: var(--bg);
    box-shadow: var(--shadow-pop);
    outline: none;
  }
  h2 {
    margin: 0 0 var(--sp-2);
    font-size: 15px;
    font-weight: 600;
  }
  p {
    margin: 0 0 var(--sp-3);
    color: var(--muted);
  }
  .tabs {
    display: grid;
    gap: var(--sp-1);
    max-height: 40vh;
    margin: 0 0 var(--sp-5);
    padding: 0;
    overflow-y: auto;
    list-style: none;
  }
  .tabs li {
    display: flex;
    align-items: baseline;
    gap: var(--sp-2);
    min-width: 0;
  }
  .name {
    overflow: hidden;
    font-weight: 600;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .doc {
    overflow: hidden;
    color: var(--muted);
    font-size: var(--fs-ui-sm);
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  footer {
    display: flex;
    flex-wrap: wrap;
    justify-content: flex-end;
    gap: var(--sp-2);
  }
</style>

<script lang="ts">
  // First run (a copy whose installer did not ask: macOS, Linux, the Microsoft Store): "Open these files with PerMark?",
  // Markdown checked, the other types unchecked. Asked once; Escape | backdrop = "Not now". Never asked on an update.
  import type { FileGroup } from "../../api";
  import { t, type MessageKey } from "../i18n";
  import { defaultApp } from "./defaultApp.svelte";

  const NAMES: Readonly<Record<FileGroup, MessageKey>> = {
    markdown: "fileGroup.markdown",
    text: "fileGroup.text",
    json: "fileGroup.json",
    xml: "fileGroup.xml",
    yaml: "fileGroup.yaml",
    toml: "fileGroup.toml",
    ini: "fileGroup.ini",
    properties: "fileGroup.properties",
  };

  const question = $derived(defaultApp.question);
  const handoff = $derived(question.status === "asking" && question.view.mode.mode === "handoff");

  let dialog: HTMLElement | undefined = $state();
  let yes: HTMLButtonElement | undefined = $state();

  function onkeydown(e: KeyboardEvent): void {
    if (e.key === "Escape") {
      e.preventDefault();
      void defaultApp.answer(false);
      return;
    }
    if (e.key !== "Tab" || !dialog) return;
    const focusable = Array.from(
      dialog.querySelectorAll<HTMLElement>("button:not(:disabled), input:not(:disabled)"),
    );
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
    yes?.focus();
    return () => before?.focus();
  });
</script>

{#if question.status === "asking"}
  <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
  <div
    class="backdrop"
    onclick={(e) => {
      if (e.target === e.currentTarget) void defaultApp.answer(false);
    }}
  >
    <div
      bind:this={dialog}
      class="dialog"
      role="dialog"
      aria-modal="true"
      aria-labelledby="default-app-title"
      aria-describedby="default-app-body"
      tabindex="-1"
      {onkeydown}
    >
      <h2 id="default-app-title">{t("defaultApp.askTitle")}</h2>
      <p id="default-app-body">{handoff ? t("defaultApp.askHandoff") : t("defaultApp.askBody")}</p>
      <fieldset>
        <legend class="sr-only">{t("defaultApp.askTitle")}</legend>
        {#each question.view.groups as group (group.group)}
          <label>
            <input
              type="checkbox"
              checked={question.chosen.includes(group.group)}
              onchange={(e) => defaultApp.toggleChosen(group.group, e.currentTarget.checked)}
            />
            <span class="name">{t(NAMES[group.group])}</span>
            <span class="exts">{group.extensions.join(" ")}</span>
          </label>
        {/each}
      </fieldset>
      <footer>
        <button type="button" class="btn" onclick={() => void defaultApp.answer(false)}>
          {t("defaultApp.notNow")}
        </button>
        <button
          bind:this={yes}
          type="button"
          class="btn btn--primary"
          disabled={question.chosen.length === 0}
          onclick={() => void defaultApp.answer(true)}
        >
          {handoff ? t("defaultApp.handoff") : t("defaultApp.use")}
        </button>
      </footer>
    </div>
  </div>
{/if}

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
    width: min(480px, 100%);
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
    margin: 0 0 var(--sp-2);
    font-size: 15px;
    font-weight: 600;
  }
  p {
    margin: 0 0 var(--sp-4);
    color: var(--muted);
  }
  fieldset {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: var(--sp-2) var(--sp-4);
    margin: 0 0 var(--sp-5);
    padding: 0;
    border: 0;
  }
  label {
    display: flex;
    align-items: baseline;
    gap: var(--sp-2);
    min-width: 0;
    cursor: pointer;
  }
  .name {
    white-space: nowrap;
  }
  .exts {
    min-width: 0;
    overflow: hidden;
    color: var(--faint);
    font-size: var(--fs-ui-xs);
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  footer {
    display: flex;
    flex-wrap: wrap;
    justify-content: flex-end;
    gap: var(--sp-2);
  }
  @media (max-width: 420px) {
    fieldset {
      grid-template-columns: 1fr;
    }
  }
</style>

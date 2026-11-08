<script lang="ts">
  // "Unsaved changes" before the edited document is left: Save | Discard | Keep editing. A modal question: it takes
  // the keyboard on Keep editing (the answer that loses nothing), Tab stays inside, Escape keeps editing, and a press
  // on the backdrop leaves the focus where it is, so no key reaches the editor behind. The window's shortcuts pass
  // while it is open (`core/commands/dispatch.ts`, `aria-modal`).
  import { t } from "../i18n";
  import { dialogFocus } from "../shell/dialogFocus";

  type Choice = "save" | "discard" | "keepEditing";
  type Props = {
    /** The edited document's file name. */
    name: string;
    /** Save is on offer (not for a document in conflict with the disk). */
    canSave: boolean;
    answer: (choice: Choice) => void;
  };
  let { name, canSave, answer }: Props = $props();
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div
  class="backdrop"
  onmousedown={(e) => {
    if (e.target === e.currentTarget) e.preventDefault();
  }}
>
  <!-- the answer decides where the focus goes next (Keep editing: the editor), so it is not given back here -->
  <div
    class="dialog"
    role="dialog"
    aria-modal="true"
    aria-labelledby="pm-unsaved-title"
    tabindex="-1"
    use:dialogFocus={{
      initial: "[data-initial]",
      onEscape: () => answer("keepEditing"),
      restoreFocus: false,
    }}
  >
    <h2 id="pm-unsaved-title">{t("edit.unsavedTitle")}</h2>
    <p>{t("edit.unsavedText", { name })}</p>
    <div class="buttons">
      {#if canSave}
        <button type="button" class="btn btn--primary" onclick={() => answer("save")}>
          {t("edit.save")}
        </button>
      {/if}
      <button type="button" class="btn" onclick={() => answer("discard")}>
        {t("edit.discard")}
      </button>
      <button data-initial type="button" class="btn" onclick={() => answer("keepEditing")}>
        {t("edit.keepEditing")}
      </button>
    </div>
  </div>
</div>

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    z-index: 50;
    display: grid;
    place-items: center;
    padding: var(--sp-4);
    background: var(--backdrop);
  }
  .dialog {
    width: min(440px, 100%);
    padding: var(--sp-5);
    border-radius: var(--radius-card);
    background: var(--bg);
    box-shadow: var(--shadow-pop);
    outline: none;
  }
  .dialog h2 {
    margin: 0 0 var(--sp-2);
    font-size: 15px;
  }
  .dialog p {
    margin: 0 0 var(--sp-4);
    color: var(--muted);
    overflow-wrap: anywhere;
  }
  .buttons {
    display: flex;
    flex-wrap: wrap;
    justify-content: flex-end;
    gap: var(--sp-2);
  }
</style>

<script lang="ts">
  // The field a comment is written in: a small box under the selected passage. Enter saves, Shift+Enter starts a new
  // line, Esc cancels. Clicking elsewhere closes it only while it is empty, so a comment being written is not lost.
  import { onMount } from "svelte";
  import { t } from "../../core/i18n";
  import { excerpt } from "./model";
  import { comments, MAX_CHARS } from "./store.svelte";

  let el: HTMLElement | undefined = $state();
  let input: HTMLTextAreaElement | undefined = $state();
  let text = $state("");
  let pos = $state({ left: 0, top: 0 });
  let saving = $state(false);

  const WIDTH = 360;
  const MARGIN = 8;
  /** The header of the window: a field is never put under it. */
  const TOP_EDGE = 52;

  const draft = $derived(comments.draft);

  /** Under the passage, above it when there is no room, and always inside the window. */
  function place(): void {
    const rect = draft?.rect;
    if (!el || !rect) return;
    const width = el.offsetWidth || WIDTH;
    const height = el.offsetHeight;
    const left = Math.max(MARGIN, Math.min(window.innerWidth - width - MARGIN, rect.left));
    let top = rect.bottom + 6;
    if (top + height > window.innerHeight - MARGIN) top = rect.top - height - 6;
    top = Math.max(TOP_EDGE, Math.min(window.innerHeight - height - MARGIN, top));
    pos = { left, top };
  }

  function grow(): void {
    if (!input) return;
    input.style.height = "auto";
    input.style.height = `${input.scrollHeight}px`;
    place();
  }

  async function save(): Promise<void> {
    if (saving || text.trim() === "") return;
    saving = true;
    try {
      await comments.save(text);
    } finally {
      saving = false;
    }
  }

  function onkeydown(event: KeyboardEvent): void {
    if (event.isComposing || event.keyCode === 229) return;
    if (event.key === "Escape") {
      event.preventDefault();
      event.stopPropagation();
      comments.cancel();
      return;
    }
    if (event.key !== "Enter" || event.shiftKey || event.ctrlKey || event.altKey || event.metaKey)
      return;
    event.preventDefault();
    void save();
  }

  onMount(() => {
    const before = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    input?.focus();
    place();
    const outside = (event: PointerEvent): void => {
      if (el && event.target instanceof Node && !el.contains(event.target) && text.trim() === "")
        comments.cancel();
    };
    window.addEventListener("pointerdown", outside, true);
    window.addEventListener("scroll", place, true);
    window.addEventListener("resize", place);
    return () => {
      window.removeEventListener("pointerdown", outside, true);
      window.removeEventListener("scroll", place, true);
      window.removeEventListener("resize", place);
      before?.focus();
    };
  });
</script>

{#if draft}
  <div
    class="field"
    bind:this={el}
    role="dialog"
    aria-modal="false"
    aria-label={t("comments.field.label")}
    tabindex="-1"
    style:left="{pos.left}px"
    style:top="{pos.top}px"
    style:width="min({WIDTH}px, calc(100vw - {2 * MARGIN}px))"
    {onkeydown}
  >
    <p class="quote">{excerpt(draft.quote, 120)}</p>
    <textarea
      bind:this={input}
      bind:value={text}
      rows="2"
      maxlength={MAX_CHARS}
      placeholder={t("comments.field.placeholder")}
      aria-label={t("comments.field.label")}
      oninput={grow}></textarea>
    <div class="foot">
      <span class="hint">{t("comments.field.hint")}</span>
      <button
        type="button"
        class="btn btn--sm btn--primary"
        disabled={saving || text.trim() === ""}
        onclick={() => void save()}
      >
        {t("comments.field.save")}
      </button>
    </div>
  </div>
{/if}

<style>
  .field {
    position: fixed;
    z-index: 50;
    display: flex;
    flex-direction: column;
    gap: var(--sp-1);
    padding: var(--sp-2);
    border: 1px solid var(--border-strong);
    border-radius: var(--radius-card);
    background: var(--surface);
    box-shadow: var(--shadow-pop);
    font-size: var(--fs-ui);
  }
  .quote {
    margin: 0;
    padding-left: var(--sp-2);
    border-left: 3px solid var(--accent-muted);
    color: var(--muted);
    font-size: var(--fs-ui-sm);
    overflow-wrap: anywhere;
  }
  textarea {
    width: 100%;
    max-height: 40vh;
    padding: var(--sp-1) var(--sp-2);
    border: 1px solid var(--border-strong);
    border-radius: var(--radius-control);
    background: var(--bg);
    color: var(--text);
    font: inherit;
    resize: none;
  }
  textarea:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: -1px;
  }
  .foot {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--sp-2);
  }
  .hint {
    color: var(--faint);
    font-size: var(--fs-ui-xs);
  }
</style>

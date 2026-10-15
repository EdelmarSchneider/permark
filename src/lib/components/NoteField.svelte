<script lang="ts">
  // The field a review note is written in: a small box under the selected passage. Enter saves, Shift+Enter starts a new
  // line, Esc cancels. Clicking elsewhere closes it only while it is empty, so a note that is being written is not lost.
  import { onMount } from "svelte";
  import { t } from "../i18n";
  import { registerOverlay, rememberFocus } from "../overlays";
  import { notes } from "../stores/notes.svelte";

  const MAX_CHARS = 2000;
  const WIDTH = 360;
  const MARGIN = 8;
  /** The header of the window: a field is never put under it. */
  const TOP_EDGE = 52;

  let el: HTMLElement | undefined = $state();
  let input: HTMLTextAreaElement | undefined = $state();
  let text = $state("");
  let pos = $state({ left: 0, top: 0 });
  /** Where the passage was last seen: the page may draw it again (and the range with it) while the note is written. */
  let seen: DOMRect | null = null;

  function passageRect(): DOMRect | null {
    const range = notes.draft?.target.range;
    const rect = range?.getBoundingClientRect();
    if (rect && (rect.width > 0 || rect.height > 0)) seen = rect;
    return seen ?? notes.draft?.target.rect ?? null;
  }

  /** Under the passage, above it when there is no room, and always inside the window. */
  function place(): void {
    const rect = passageRect();
    if (!el || !rect) return;
    const width = el.offsetWidth;
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

  async function onkeydown(e: KeyboardEvent): Promise<void> {
    if (e.key !== "Enter" || e.shiftKey || e.ctrlKey || e.altKey || e.metaKey || e.isComposing) {
      return;
    }
    e.preventDefault();
    await notes.save(text);
  }

  onMount(() => {
    const restore = rememberFocus();
    const unregister = registerOverlay(() => notes.cancel());
    input?.focus();
    place();
    const outside = (e: PointerEvent): void => {
      if (!el?.contains(e.target as Node) && text.trim() === "") notes.cancel();
    };
    window.addEventListener("pointerdown", outside, true);
    window.addEventListener("scroll", place, true);
    window.addEventListener("resize", place);
    return () => {
      window.removeEventListener("pointerdown", outside, true);
      window.removeEventListener("scroll", place, true);
      window.removeEventListener("resize", place);
      unregister();
      restore();
    };
  });
</script>

<div
  bind:this={el}
  class="note-field"
  role="dialog"
  aria-label={t("notes.field.label")}
  style:left="{pos.left}px"
  style:top="{pos.top}px"
  style:width="{WIDTH}px"
>
  <textarea
    bind:this={input}
    bind:value={text}
    rows="2"
    maxlength={MAX_CHARS}
    placeholder={t("notes.field.placeholder")}
    aria-label={t("notes.field.label")}
    disabled={notes.saving}
    {onkeydown}
    oninput={grow}></textarea>
  <span class="note-field__hint">{t("notes.field.hint")}</span>
</div>

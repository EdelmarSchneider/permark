<script lang="ts">
  // The link popover of Visual mode (Ctrl+K, or the link button): the text and the target of a link. The target offers
  // files in the folder, and headings of this document after #. A target that points to a file the folder does not have
  // is flagged. Enter applies, Esc closes. Down and then Enter picks a suggestion instead of applying.
  import { onMount } from "svelte";
  import { api } from "../api";
  import { linkSuggestions } from "../editor/complete";
  import { t } from "../i18n";
  import type { LinkTarget } from "../editor/visual/link";
  import { vault } from "../stores/vault.svelte";
  import Popover, { type Anchor } from "./Popover.svelte";

  type Props = {
    anchor: Anchor;
    target: LinkTarget;
    /** The document the link is written in (relative targets are looked up from it). */
    docPath: string;
    /** Headings of the document, offered after `#`. */
    headings?: readonly string[];
    onapply(next: { text: string; href: string }): void;
    onremove(): void;
    onclose(): void;
  };
  let { anchor, target, docPath, headings = [], onapply, onremove, onclose }: Props = $props();

  // svelte-ignore state_referenced_locally
  let text = $state(target.text);
  // svelte-ignore state_referenced_locally
  let href = $state(target.href);
  let broken = $state(false);
  let hrefInput: HTMLInputElement | undefined = $state();
  let textInput: HTMLInputElement | undefined = $state();
  // svelte-ignore state_referenced_locally
  let caret = $state(target.href.length);
  let pick = $state(-1);

  const suggestions = $derived(
    linkSuggestions(href.slice(0, caret), {
      fromFile: docPath,
      files: vault.files,
      headings,
      wrap: false,
    }),
  );

  $effect(() => {
    void href;
    pick = -1;
  });

  function syncCaret(e: Event): void {
    const input = e.currentTarget as HTMLInputElement;
    caret = input.selectionStart ?? href.length;
  }

  function onTargetKey(e: KeyboardEvent): void {
    if (suggestions.length === 0) return;
    if (e.key === "ArrowDown") {
      e.preventDefault();
      pick = Math.min(suggestions.length - 1, pick + 1);
    } else if (e.key === "ArrowUp") {
      if (pick < 0) return;
      e.preventDefault();
      pick -= 1;
    } else if (e.key === "Enter" && pick >= 0) {
      e.preventDefault();
      const item = suggestions[pick];
      href = item.insert;
      caret = item.insert.length;
    }
  }

  function useSuggestion(insert: string): void {
    href = insert;
    caret = insert.length;
    hrefInput?.focus();
  }

  // a link to a file that is not there: asked of the backend a moment after typing stops
  $effect(() => {
    const value = href.trim();
    broken = false;
    if (value === "" || value.startsWith("#")) return;
    const timer = setTimeout(() => {
      void api
        .resolveLink(docPath, value)
        .then((result) => {
          if (href.trim() === value) broken = result.kind === "missing";
        })
        .catch(() => {});
    }, 300);
    return () => clearTimeout(timer);
  });

  onMount(() => {
    // the text is where to start for a link that is being made out of nothing, the target otherwise
    const timer = setTimeout(() => (target.text === "" ? textInput : hrefInput)?.focus(), 0);
    return () => clearTimeout(timer);
  });

  function submit(e: SubmitEvent): void {
    e.preventDefault();
    onapply({ text, href });
  }
</script>

<Popover {anchor} variant="panel" label={t("link.edit")} {onclose}>
  <form class="link-form" onsubmit={submit}>
    <label class="field">
      <span class="field__label">{t("link.text")}</span>
      <input
        bind:this={textInput}
        bind:value={text}
        class="text-input"
        type="text"
        autocomplete="off"
        spellcheck="false"
      />
    </label>
    <label class="field">
      <span class="field__label">{t("link.target")}</span>
      <input
        bind:this={hrefInput}
        bind:value={href}
        class="text-input"
        type="text"
        autocomplete="off"
        spellcheck="false"
        placeholder={t("link.targetPlaceholder")}
        aria-invalid={broken}
        aria-autocomplete="list"
        aria-controls="link-suggestions"
        oninput={syncCaret}
        onkeyup={syncCaret}
        onclick={syncCaret}
        onkeydown={onTargetKey}
      />
      {#if suggestions.length > 0}
        <div
          id="link-suggestions"
          class="link-suggest"
          role="listbox"
          aria-label={t("complete.label")}
        >
          {#each suggestions as item, i (`${item.kind}:${item.insert}`)}
            <button
              type="button"
              role="option"
              class={["suggest__item", i === pick && "is-active"]}
              aria-selected={i === pick}
              onmousedown={(e) => e.preventDefault()}
              onmousemove={() => (pick = i)}
              onclick={() => useSuggestion(item.insert)}
            >
              <span class="suggest__label">{item.label}</span>
              {#if item.detail}<span class="suggest__detail">{item.detail}</span>{/if}
            </button>
          {/each}
        </div>
      {/if}
      <span class="field__hint field__hint--warn" role="status"
        >{broken ? t("link.broken") : ""}</span
      >
    </label>
    <div class="link-form__actions">
      {#if target.existing}
        <button type="button" class="btn btn--sm btn--ghost" onclick={onremove}>
          {t("link.remove")}
        </button>
      {/if}
      <span class="spacer"></span>
      <button type="submit" class="btn btn--sm btn--primary">{t("link.apply")}</button>
    </div>
  </form>
</Popover>

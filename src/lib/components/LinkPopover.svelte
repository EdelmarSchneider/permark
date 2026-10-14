<script lang="ts">
  // The link popover of Visual mode (Ctrl+K, or the link button): the text and the target of a link. No autocomplete. A target that
  // points to a file the folder does not have is flagged. Enter applies, Esc closes.
  import { onMount } from "svelte";
  import { api } from "../api";
  import { t } from "../i18n";
  import type { LinkTarget } from "../editor/visual/link";
  import Popover, { type Anchor } from "./Popover.svelte";

  type Props = {
    anchor: Anchor;
    target: LinkTarget;
    /** The document the link is written in (relative targets are looked up from it). */
    docPath: string;
    onapply(next: { text: string; href: string }): void;
    onremove(): void;
    onclose(): void;
  };
  let { anchor, target, docPath, onapply, onremove, onclose }: Props = $props();

  // svelte-ignore state_referenced_locally
  let text = $state(target.text);
  // svelte-ignore state_referenced_locally
  let href = $state(target.href);
  let broken = $state(false);
  let hrefInput: HTMLInputElement | undefined = $state();
  let textInput: HTMLInputElement | undefined = $state();

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
      />
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

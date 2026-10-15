<script lang="ts">
  import { onMount } from "svelte";
  import { selectionIn } from "../dom/domUtil";
  import { keys, t } from "../i18n";
  import { copyText } from "../platform";
  import { app } from "../stores/app.svelte";
  import { docs } from "../stores/docs.svelte";
  import { comments } from "../stores/comments.svelte";
  import { ui } from "../stores/ui.svelte";
  import { vault } from "../stores/vault.svelte";
  import Icon from "./Icon.svelte";

  const SHOW_DELAY_MS = 150;

  let el: HTMLElement | undefined = $state();
  let info = $state<{ text: string; section: string | null; rect: DOMRect } | null>(null);
  let pos = $state({ left: 0, top: 0 });
  let timer: ReturnType<typeof setTimeout> | null = null;

  /** A comment can be left on a passage of a Markdown document of a project. */
  const canComment = $derived(
    !vault.isSimple && docs.current?.kind === "markdown" && !docs.editing,
  );

  function hide(): void {
    if (timer !== null) clearTimeout(timer);
    info = null;
  }

  function bodyEl(): HTMLElement | null {
    return document.querySelector<HTMLElement>(".md-body");
  }

  function place(): void {
    if (!el || !info) return;
    const w = el.offsetWidth;
    const h = el.offsetHeight;
    const left = Math.max(
      8,
      Math.min(window.innerWidth - w - 8, info.rect.left + info.rect.width / 2 - w / 2),
    );
    const above = info.rect.top - h - 8;
    pos = { left, top: above < 52 ? info.rect.bottom + 8 : above };
  }

  $effect(() => {
    if (info) place();
  });

  onMount(() => {
    const onMouseUp = (e: MouseEvent): void => {
      if (el?.contains(e.target as Node)) return;
      if (timer !== null) clearTimeout(timer);
      timer = setTimeout(() => {
        const body = bodyEl();
        info = body ? selectionIn(body) : null;
        // a comment asked for from somewhere that takes the selection away is written on this one
        if (info) comments.remember();
      }, SHOW_DELAY_MS);
    };
    const onMouseDown = (e: MouseEvent): void => {
      if (!el?.contains(e.target as Node)) hide();
    };
    const onKey = (e: KeyboardEvent): void => {
      if (e.key === "Escape" && info) hide();
    };
    document.addEventListener("mouseup", onMouseUp);
    document.addEventListener("mousedown", onMouseDown);
    document.addEventListener("keydown", onKey);
    window.addEventListener("scroll", hide, true);
    return () => {
      document.removeEventListener("mouseup", onMouseUp);
      document.removeEventListener("mousedown", onMouseDown);
      document.removeEventListener("keydown", onKey);
      window.removeEventListener("scroll", hide, true);
      if (timer !== null) clearTimeout(timer);
    };
  });

  async function copyWithReference(): Promise<void> {
    if (!info) return;
    await app.copyQuote(info.text, info.section, "selection");
  }

  function addComment(): void {
    hide();
    comments.begin();
  }

  async function copyPlain(): Promise<void> {
    if (!info) return;
    if (await copyText(info.text)) ui.flash("selection-plain");
  }
</script>

{#if info}
  <div
    bind:this={el}
    class="popover"
    role="toolbar"
    aria-label={t("selection.label")}
    style:left="{pos.left}px"
    style:top="{pos.top}px"
  >
    <button type="button" class="btn" onclick={copyWithReference}>
      <Icon name={ui.flashed === "selection" ? "check" : "at"} size="sm" />
      <span
        >{ui.flashed === "selection" ? t("common.copied") : t("selection.copyWithReference")}</span
      >
      <kbd class="hint-kbd">{keys("Ctrl+Alt+C")}</kbd>
    </button>
    <span class="divider"></span>
    <button type="button" class="btn" onclick={copyPlain}>
      <Icon name={ui.flashed === "selection-plain" ? "check" : "copy"} size="sm" />
      <span>{ui.flashed === "selection-plain" ? t("common.copied") : t("selection.copy")}</span>
    </button>
    {#if canComment}
      <span class="divider"></span>
      <button type="button" class="btn" title={t("comments.addHint")} onclick={addComment}>
        <Icon name="comment" size="sm" />
        <span>{t("comments.add")}</span>
        <kbd class="hint-kbd">{keys("N")}</kbd>
      </button>
    {/if}
  </div>
{/if}

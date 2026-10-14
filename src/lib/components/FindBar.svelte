<script lang="ts">
  import { onMount, untrack } from "svelte";
  import { bridge } from "../bridge";
  import { t } from "../i18n";
  import { registerOverlay } from "../overlays";
  import { ui } from "../stores/ui.svelte";
  import IconButton from "./IconButton.svelte";

  let input: HTMLInputElement | undefined = $state();
  let timer: ReturnType<typeof setTimeout> | null = null;

  const empty = $derived(ui.find.query.trim() !== "" && ui.find.total === 0);
  const counter = $derived(
    ui.find.query.trim() === ""
      ? ""
      : ui.find.total === 0
        ? t("find.none")
        : t("find.count", { index: ui.find.index + 1, total: ui.find.total }),
  );

  function close(): void {
    if (timer !== null) clearTimeout(timer);
    bridge.find?.clear();
    ui.find.open = false;
    ui.find.total = 0;
    ui.find.index = -1;
    document.querySelector<HTMLElement>(".doc-scroll")?.focus();
  }

  function oninput(e: Event & { currentTarget: HTMLInputElement }): void {
    ui.find.query = e.currentTarget.value;
    ui.find.mode = "phrase";
    if (timer !== null) clearTimeout(timer);
    timer = setTimeout(() => {
      timer = null;
      bridge.find?.run(ui.find.query, ui.find.mode);
    }, 100);
  }

  function onkeydown(e: KeyboardEvent): void {
    if (e.key === "Enter") {
      e.preventDefault();
      if (timer !== null) {
        clearTimeout(timer);
        timer = null;
        bridge.find?.run(ui.find.query, ui.find.mode);
      } else {
        bridge.find?.step(e.shiftKey ? -1 : 1);
      }
    }
  }

  onMount(() => {
    const unregister = registerOverlay(close);
    if (ui.find.takeFocus) {
      input?.focus();
      input?.select();
    }
    return unregister;
  });

  // Ctrl+F while the bar is open: focus and select again
  $effect(() => {
    void ui.find.focusTick;
    untrack(() => {
      if (ui.find.takeFocus) {
        input?.focus();
        input?.select();
      }
    });
  });
</script>

<div class="findbar" role="search">
  <input
    bind:this={input}
    value={ui.find.query}
    {oninput}
    {onkeydown}
    placeholder={t("find.placeholder")}
    aria-label={t("find.label")}
    spellcheck="false"
    autocomplete="off"
  />
  <span class={["findbar__count", empty && "is-empty"]} aria-live="polite">{counter}</span>
  <IconButton
    small
    icon="chevron-up"
    label={t("find.previous")}
    hint={t("find.previousHint")}
    disabled={ui.find.total === 0}
    onclick={() => bridge.find?.step(-1)}
  />
  <IconButton
    small
    icon="chevron-down"
    label={t("find.next")}
    hint={t("find.nextHint")}
    disabled={ui.find.total === 0}
    onclick={() => bridge.find?.step(1)}
  />
  <IconButton small icon="x" label={t("find.close")} hint={t("find.closeHint")} onclick={close} />
</div>

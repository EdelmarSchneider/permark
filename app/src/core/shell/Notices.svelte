<script lang="ts">
  // Quiet notices (bottom right): failed opens, state set aside, settings not kept. Dismissable; never block. Timed
  // ones close by themselves (paused while the pointer | focus is on them). Their height is published as
  // `--notices-h` so scrolling pages keep room under their last control.
  import { apiErrorText, ipcErrorText } from "../errors";
  import { t } from "../i18n";
  import type { Notice, WindowStore } from "../window.svelte";

  type Props = { store: WindowStore };
  let { store }: Props = $props();

  let region: HTMLElement | undefined = $state();

  // the room the notices take at the bottom (0 when none): pages pad their end by it
  $effect(() => {
    const el = region;
    const root = document.documentElement;
    if (!el) {
      root.style.removeProperty("--notices-h");
      return;
    }
    const set = (): void => {
      const box = el.getBoundingClientRect();
      root.style.setProperty("--notices-h", `${Math.ceil(window.innerHeight - box.top)}px`);
    };
    set();
    const observer = typeof ResizeObserver === "undefined" ? null : new ResizeObserver(set);
    observer?.observe(el);
    return () => {
      observer?.disconnect();
      root.style.removeProperty("--notices-h");
    };
  });

  function text(notice: Notice): string {
    switch (notice.kind) {
      case "openFailed":
        return t("welcome.openFailed", { path: notice.path, message: ipcErrorText(notice.error) });
      case "apiFailed":
        return notice.path === null
          ? apiErrorText(notice.error)
          : t("welcome.openFailed", { path: notice.path, message: apiErrorText(notice.error) });
      case "stateRecovered":
        return t("notice.stateRecovered", { file: notice.notice.file });
      case "settingNotSaved":
        return t("settings.notSaved", { message: apiErrorText(notice.error) });
      case "initFailed":
        return t("project.initFailed", { path: notice.path, message: apiErrorText(notice.error) });
      case "fileOpFailed":
        return apiErrorText(notice.error);
      case "note":
      case "action":
      case "timed":
        return notice.text;
      default:
        return notice satisfies never;
    }
  }
</script>

{#if store.notices.length > 0}
  <div class="notices" role="status" aria-live="polite" bind:this={region}>
    {#each store.notices as notice (notice.id)}
      <div
        role="group"
        class="notice"
        onpointerenter={() => store.holdNotice(notice.id, "pointer")}
        onpointerleave={() => store.releaseNotice(notice.id, "pointer")}
        onfocusin={() => store.holdNotice(notice.id, "focus")}
        onfocusout={(e) => {
          const into = e.relatedTarget;
          if (!(into instanceof Node && e.currentTarget.contains(into))) {
            store.releaseNotice(notice.id, "focus");
          }
        }}
      >
        <span class="text">{text(notice)}</span>
        {#if notice.kind === "action" || notice.kind === "timed"}
          {#each notice.actions as action (action.label)}
            <button
              type="button"
              class="btn btn--sm"
              onclick={() => {
                store.dismiss(notice.id, "action");
                action.run();
              }}
            >
              {action.label}
            </button>
          {/each}
        {/if}
        <button
          type="button"
          class="btn btn--ghost btn--sm"
          onclick={() => store.dismiss(notice.id)}
        >
          {t("common.dismiss")}
        </button>
      </div>
    {/each}
  </div>
{/if}

<style>
  .notices {
    position: fixed;
    right: var(--sp-4);
    bottom: calc(var(--h-status) + var(--sp-3));
    z-index: 45;
    display: flex;
    flex-direction: column;
    gap: var(--sp-2);
    width: min(420px, calc(100vw - 2 * var(--sp-4)));
  }
  .notice {
    display: flex;
    align-items: center;
    gap: var(--sp-3);
    padding: var(--sp-2) var(--sp-2) var(--sp-2) var(--sp-3);
    border: 1px solid var(--border);
    border-radius: var(--radius-card);
    background: var(--bg);
    box-shadow: var(--shadow-pop);
    animation: pm-fade var(--dur) var(--ease);
  }
  .text {
    flex: 1;
    min-width: 0;
    overflow-wrap: anywhere;
  }
</style>

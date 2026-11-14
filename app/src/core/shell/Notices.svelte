<script lang="ts">
  // Quiet notices (bottom right of the page, left of the right panel so they never cover its lists): failed opens,
  // state set aside, settings not kept, what a file operation did. Dismissable; never block. Most close by themselves
  // (paused while the pointer | focus is on them). Esc with the keyboard in one dismisses it. Their height is published
  // as `--notices-h` so scrolling pages keep room under their last control. A notice's button that ran takes the
  // notice away: the keyboard that was on it goes back where it came from, never to the page body.
  import { tick } from "svelte";
  import { apiErrorText, ipcErrorText } from "../errors";
  import { t } from "../i18n";
  import type { Notice, NoticeAction, WindowStore } from "../window.svelte";
  import { returnFocus } from "./sides";

  type Props = {
    store: WindowStore;
    /** Width of the right panel on screen (0: none): the notices keep to its left. */
    rightInset?: number;
  };
  let { store, rightInset = 0 }: Props = $props();

  let region: HTMLElement | undefined = $state();
  /** What had the keyboard before it came into the notices (`null`: nothing did, e.g. a press with the pointer). */
  let cameFrom: Element | null = null;

  /** The focus came into a notice: from outside the notices, what had it is remembered. */
  function entered(event: FocusEvent): void {
    const from = event.relatedTarget;
    if (from instanceof Node && region?.contains(from)) return;
    cameFrom = from instanceof Element ? from : null;
  }

  /**
   * A notice's button was pressed: the notice goes, its action runs. The button that had the keyboard is gone with
   * the notice: once the action is done (its own work too, when it says so) and what it changed is drawn, the
   * keyboard is where the action put it, else back on what had it before the notices, else (that is gone too) on
   * the page.
   */
  async function act(id: number, action: NoticeAction): Promise<void> {
    const before = cameFrom;
    store.dismiss(id, "action");
    try {
      await action.run();
    } finally {
      await tick();
      returnFocus(before);
    }
  }

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
        // no backup: left in place, never written over (state files, board item 15)
        return notice.notice.backup === null
          ? t("notice.stateUnreadable", { file: notice.notice.file })
          : t("notice.stateRecovered", { file: notice.notice.file });
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
  <div
    class="notices"
    role="status"
    aria-live="polite"
    style:--notices-right="{rightInset}px"
    bind:this={region}
  >
    {#each store.notices as notice (notice.id)}
      <!-- Esc bubbling up from its own buttons: the group only hears it, it never takes the keyboard itself -->
      <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
      <div
        role="group"
        class="notice"
        onpointerenter={() => store.holdNotice(notice.id, "pointer")}
        onpointerleave={() => store.releaseNotice(notice.id, "pointer")}
        onfocusin={(e) => {
          entered(e);
          store.holdNotice(notice.id, "focus");
        }}
        onkeydown={(e) => {
          if (e.key !== "Escape" || e.defaultPrevented) return;
          // this notice, nothing behind it (a review, the page)
          e.preventDefault();
          e.stopPropagation();
          store.dismiss(notice.id);
        }}
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
            <button type="button" class="btn btn--sm" onclick={() => void act(notice.id, action)}>
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
    right: calc(var(--notices-right, 0px) + var(--sp-4));
    bottom: calc(var(--h-status) + var(--sp-3));
    z-index: 45;
    display: flex;
    flex-direction: column;
    gap: var(--sp-2);
    width: min(420px, calc(100vw - var(--notices-right, 0px) - 2 * var(--sp-4)));
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
  @media (prefers-reduced-motion: reduce) {
    .notice {
      animation: none;
    }
  }
</style>

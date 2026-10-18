<script lang="ts">
  import { tick, untrack } from "svelte";
  import { formatNumber, t, tn } from "../i18n";
  import { app } from "../stores/app.svelte";
  import { brokenLinks } from "../stores/brokenLinks.svelte";
  import { ui } from "../stores/ui.svelte";
  import type { BrokenLink } from "../types";
  import Icon from "./Icon.svelte";
  import IconButton from "./IconButton.svelte";

  const report = $derived(brokenLinks.report);
  const groups = $derived(groupByDocument(report?.links ?? []));

  // Ctrl+Shift+B: the list takes the keyboard
  $effect(() => {
    void ui.linksFocusTick;
    untrack(() => {
      if (ui.linksFocusTick > 0) {
        void tick().then(() => {
          document.querySelector<HTMLElement>(".links-scroll")?.focus();
        });
      }
    });
  });

  function groupByDocument(links: BrokenLink[]): { path: string; items: BrokenLink[] }[] {
    const out: { path: string; items: BrokenLink[] }[] = [];
    for (const link of links) {
      const last = out.at(-1);
      if (last && last.path === link.path) last.items.push(link);
      else out.push({ path: link.path, items: [link] });
    }
    return out;
  }

  function open(path: string, line?: number): void {
    void app.openDoc(path, line === undefined ? {} : { locate: { line, find: null } });
  }

  function repair(link: BrokenLink, target: string): void {
    void brokenLinks.repair(link.path, link.start, link.end, target);
  }
</script>

<div class="links-panel">
  <div class="panel-header">
    <span class="panel-title">{t("links.title")}</span>
    <span class="spacer"></span>
    {#if report && report.total > 0}
      <span class="links-count">{tn("links.summary", report.total)}</span>
    {/if}
    <IconButton
      icon="refresh"
      label={t("links.refresh")}
      onclick={() => void brokenLinks.refresh()}
    />
  </div>

  {#if brokenLinks.status === "loading" && report === null}
    <p class="links-note">{t("links.looking")}</p>
  {:else if brokenLinks.status === "error"}
    <p class="links-note">{brokenLinks.error}</p>
  {:else if report && report.total === 0}
    <div class="links-empty">
      <Icon name="link-off" />
      <strong>{t("links.empty")}</strong>
    </div>
  {:else if report}
    <div class="links-scroll scroll" tabindex="-1">
      {#if report.truncated}
        <p class="links-note">{t("links.truncated", { n: formatNumber(report.links.length) })}</p>
      {/if}
      {#each groups as group (group.path)}
        <section class="links-group">
          <button
            type="button"
            class="links-doc"
            title={group.path}
            onclick={() => open(group.path)}
          >
            {group.path}
          </button>
          {#each group.items as link (`${link.start}:${link.written}`)}
            <div class="links-row">
              <button
                type="button"
                class="links-open"
                title={t("links.openHint", { line: link.line })}
                onclick={() => open(link.path, link.line)}
              >
                <span class="links-line">{link.line}</span>
                <span class="links-dest">{link.written}</span>
              </button>
              <div class="links-actions">
                {#if link.repairs.length === 1}
                  {@const target = link.repairs[0]}
                  <button
                    type="button"
                    class="btn links-repair"
                    title={t("links.repairHint", { path: target })}
                    disabled={brokenLinks.repairing !== null}
                    onclick={() => repair(link, target)}
                  >
                    {t("links.repair")}
                  </button>
                {:else if link.repairs.length > 1}
                  {#each link.repairs as target (target)}
                    <button
                      type="button"
                      class="btn links-repair"
                      title={t("links.repairHint", { path: target })}
                      disabled={brokenLinks.repairing !== null}
                      onclick={() => repair(link, target)}
                    >
                      {target}
                    </button>
                  {/each}
                {:else}
                  <span class="links-none">{t("links.noMatch")}</span>
                {/if}
              </div>
            </div>
          {/each}
        </section>
      {/each}
      {#if report.skipped > 0}
        <p class="links-note">{tn("links.skipped", report.skipped)}</p>
      {/if}
    </div>
  {/if}
</div>

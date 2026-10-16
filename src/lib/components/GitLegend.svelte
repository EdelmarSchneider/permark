<script lang="ts">
  // "What do these letters mean?": the git badges in plain words. Opened from the status bar and from the Explorer menu.
  import { legendBaselineNote, legendRows } from "../gitLabels";
  import { t } from "../i18n";
  import { ui } from "../stores/ui.svelte";
  import { vault } from "../stores/vault.svelte";
  import Popover from "./Popover.svelte";

  const open = $derived(ui.gitLegend);
  const baseline = $derived(vault.git?.baseline.kind ?? "lastVisit");
  const rows = $derived(legendRows(baseline));
  const note = $derived(legendBaselineNote(baseline));
</script>

{#if open}
  <Popover
    anchor={open.anchor}
    variant="panel"
    wide
    scroll
    side={open.from === "status" ? "top" : "right"}
    label={t("git.legend.title")}
    onclose={() => (ui.gitLegend = null)}
  >
    <h3 class="git-legend__title">{t("git.legend.title")}</h3>
    <ul class="git-legend">
      {#each rows as row (row.badge.letter)}
        <li>
          <span class={["badge", row.badge.className]} aria-hidden="true">{row.badge.letter}</span>
          <span>{row.text}</span>
        </li>
      {/each}
      <li>
        <span class="rollup git-legend__dot" aria-hidden="true"></span>
        <span>{t("git.legend.folder")}</span>
      </li>
    </ul>
    <p class="git-legend__note">{note}</p>
    <p class="git-legend__note git-legend__note--promise">{t("git.legend.readOnly")}</p>
  </Popover>
{/if}

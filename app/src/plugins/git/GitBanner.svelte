<script lang="ts">
  // A strip under the header while the repository is in the middle of something (merging, rebasing, ...): PerMark
  // only reads git, so the strip says what is going on and who finishes it.
  import { t } from "../../core/i18n";
  import { git } from "./store.svelte";

  const BANNERS = {
    clean: null,
    merging: "git.banner.merging",
    rebasing: "git.banner.rebasing",
    cherryPicking: "git.banner.cherryPicking",
    reverting: "git.banner.reverting",
    bisecting: "git.banner.bisecting",
    busy: null,
  } as const;
  const key = $derived(git.snapshot === null ? null : BANNERS[git.snapshot.state]);
</script>

{#if key}
  <div class="banner" role="status">{t(key)}</div>
{/if}

<style>
  .banner {
    padding: var(--sp-2) var(--sp-4);
    border-bottom: 1px solid var(--border);
    background: color-mix(in srgb, var(--git-modified) 10%, var(--bg));
    color: var(--git-modified-text);
    font-size: var(--fs-ui-sm);
    text-align: center;
  }
</style>

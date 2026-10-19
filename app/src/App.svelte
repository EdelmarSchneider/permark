<script lang="ts">
  import { onMount } from "svelte";
  import { api, type AppInfo } from "./api";
  import { language, t } from "./core/i18n";
  import { watchSystemTheme } from "./core/theme";

  type InfoState =
    { status: "loading" } | { status: "ready"; info: AppInfo } | { status: "failed" };

  let info = $state<InfoState>({ status: "loading" });

  const versionText = $derived.by(() => {
    switch (info.status) {
      case "loading":
        return t("app.loading");
      case "ready":
        return t("status.version", { version: info.info.version });
      case "failed":
        return t("status.versionUnavailable");
      default:
        return info satisfies never;
    }
  });

  onMount(() => {
    void api.appInfo().then((result) => {
      info = result.ok ? { status: "ready", info: result.value } : { status: "failed" };
    });
  });

  $effect(() => watchSystemTheme());
  $effect(() => language.watchSystem());
</script>

<div class="frame">
  <header class="header">
    <span class="title">{t("app.name")}</span>
  </header>
  <main class="content">
    <p class="tagline">{t("welcome.tagline")}</p>
  </main>
  <footer class="status">
    <span class="spacer"></span>
    <span>{versionText}</span>
  </footer>
</div>

<style>
  .frame {
    display: grid;
    grid-template-rows: var(--h-header) minmax(0, 1fr) var(--h-status);
    height: 100%;
  }
  .header {
    display: flex;
    align-items: center;
    padding: 0 var(--sp-4);
    background: var(--surface);
    border-bottom: 1px solid var(--border);
  }
  .title {
    font-weight: 600;
  }
  .content {
    display: flex;
    align-items: center;
    justify-content: center;
    min-height: 0;
    overflow: auto;
    padding: var(--sp-6);
  }
  .tagline {
    margin: 0;
    max-width: var(--measure);
    font: var(--fs-reading) / var(--lh-reading) var(--font-reading);
    color: var(--muted);
    text-align: center;
  }
  .status {
    display: flex;
    align-items: center;
    gap: var(--sp-3);
    padding: 0 var(--sp-3);
    font-size: var(--fs-ui-xs);
    color: var(--muted);
    background: var(--surface);
    border-top: 1px solid var(--border);
  }
  .spacer {
    flex: 1;
  }
</style>

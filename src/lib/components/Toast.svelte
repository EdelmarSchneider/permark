<script lang="ts">
  import { t, tn } from "../i18n";
  import { basename, folderLabel, splitWorktree } from "../paths";
  import { app } from "../stores/app.svelte";
  import { clock } from "../stores/clock.svelte";
  import { toast } from "../stores/toast.svelte";
  import { relativeTime } from "../time";
  import Icon from "./Icon.svelte";
  import IconButton from "./IconButton.svelte";

  const current = $derived(toast.current);

  function meta(path: string, atMs: number): string {
    const { worktree } = splitWorktree(path);
    const folder = folderLabel(path) || t("feed.root");
    return `${worktree ? `@wt ${worktree} · ` : ""}${folder} · ${relativeTime(atMs, clock.now)}`;
  }
</script>

{#if current}
  <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
  <div
    class={[
      "toast",
      current.type === "message" && current.tone === "warn" && "toast--warn",
      toast.leaving && "is-leaving",
    ]}
    role={current.type === "message" && current.tone === "warn" ? "alert" : "status"}
    onmouseenter={() => toast.pause()}
    onmouseleave={() => toast.resume()}
  >
    <span class="toast__icon">
      <Icon
        name={current.type === "message"
          ? current.tone === "warn"
            ? "alert"
            : "info"
          : "file-text"}
        size="sm"
      />
    </span>
    {#if current.type === "change"}
      {@const name = basename(current.path)}
      <div style="min-width:0">
        <div class="toast__title">
          {#if current.count > 1}
            {tn("toast.docsChanged", current.count)} · {t("toast.latest")}
            <span style="font-weight:600">{name}</span>
          {:else}
            <span style="font-weight:600">{name}</span>
            {t(`toast.kind.${current.kind}`)}
          {/if}
        </div>
        <div class="toast__meta">{meta(current.path, current.atMs)}</div>
      </div>
      {#if current.kind !== "removed"}
        <button
          type="button"
          class="btn btn--ghost btn--sm btn--accent-text"
          onclick={() => {
            const path = current.path;
            toast.dismiss();
            void app.openChanged(path);
          }}
        >
          {t("common.open")}
        </button>
      {:else}
        <span></span>
      {/if}
    {:else}
      <div style="min-width:0">
        <div class="toast__title" style="white-space:normal">{current.text}</div>
      </div>
      <span></span>
    {/if}
    <IconButton small icon="x" label={t("common.dismiss")} onclick={() => toast.dismiss()} />
  </div>
{/if}

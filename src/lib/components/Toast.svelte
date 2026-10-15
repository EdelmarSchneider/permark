<script lang="ts">
  import { t, tn } from "../i18n";
  import { basename, folderLabel, splitWorktree } from "../paths";
  import { app } from "../stores/app.svelte";
  import { clock } from "../stores/clock.svelte";
  import { toast } from "../stores/toast.svelte";
  import { updates } from "../stores/updates.svelte";
  import { describeUpdate, updatePercent } from "../updates";
  import { relativeTime } from "../time";
  import Icon from "./Icon.svelte";
  import IconButton from "./IconButton.svelte";

  const current = $derived(toast.current);
  const update = $derived(updates.state);
  const installing = $derived(update.phase === "downloading" || update.phase === "installing");

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
          : current.type === "update"
            ? "download"
            : "file-text"}
        size="sm"
      />
    </span>
    {#if current.type === "change"}
      {@const name = basename(current.path)}
      {@const full =
        current.count > 1
          ? `${tn("toast.docsChanged", current.count)} · ${t("toast.latest")} ${name}`
          : `${name} ${t(`toast.kind.${current.kind}`)}`}
      <div style="min-width:0">
        <div class="toast__title" title={full}>
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
    {:else if current.type === "update"}
      <div style="min-width:0">
        <div class="toast__title" aria-live="polite">{describeUpdate(update)}</div>
        {#if installing}
          {@const percent = updatePercent(update)}
          <div
            class="toast__progress"
            role="progressbar"
            aria-label={describeUpdate(update)}
            aria-valuemin="0"
            aria-valuemax="100"
            aria-valuenow={percent ?? undefined}
          >
            <span class={[percent === null && "is-indeterminate"]} style:width="{percent ?? 100}%"
            ></span>
          </div>
        {:else}
          <div class="toast__actions">
            <button
              type="button"
              class="btn btn--primary btn--sm"
              onclick={() => void updates.install()}
            >
              {t("update.restart")}
            </button>
            <button type="button" class="btn btn--ghost btn--sm" onclick={() => toast.dismiss()}>
              {t("update.later")}
            </button>
          </div>
        {/if}
      </div>
      <span></span>
    {:else}
      <div style="min-width:0">
        <div class="toast__title" style="white-space:normal">{current.text}</div>
      </div>
      {#if current.action}
        <div class="toast__buttons">
          <button
            type="button"
            class="btn btn--ghost btn--sm btn--accent-text"
            onclick={() => toast.runAction()}
          >
            {current.action.label}
          </button>
          {#if current.second}
            <button
              type="button"
              class="btn btn--ghost btn--sm btn--accent-text"
              onclick={() => toast.runAction("second")}
            >
              {current.second.label}
            </button>
          {/if}
        </div>
      {:else}
        <span></span>
      {/if}
    {/if}
    {#if current.type === "update" && installing}
      <span></span>
    {:else}
      <IconButton small icon="x" label={t("common.dismiss")} onclick={() => toast.dismiss()} />
    {/if}
  </div>
{/if}

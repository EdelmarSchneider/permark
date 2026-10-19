<script lang="ts">
  // Git cannot be used here: the chosen git is missing, or the repository cannot be read. Said once, quietly, in the
  // status bar, with the way out (the setting). A folder that simply has no repository shows nothing.
  import TriangleAlert from "@lucide/svelte/icons/triangle-alert";
  import { conditions } from "../../core/conditions/context.svelte";
  import { t } from "../../core/i18n";
  import { ui } from "../../core/shell/ui.svelte";

  const problem = $derived.by(() => {
    const git = conditions.current.git;
    switch (git.status) {
      case "unavailable":
        return { text: t("git.unavailable"), hint: t("git.unavailableHint") };
      case "error":
        return {
          text: t("git.error"),
          hint: `${git.reason === "dubious ownership" ? t("git.errorOwnership") : git.reason} ${t("git.errorHint")}`,
        };
      case "noRepo":
      case "repo":
        return null;
      default:
        return git satisfies never;
    }
  });
</script>

{#if problem}
  <button type="button" class="problem" title={problem.hint} onclick={() => ui.openSettings()}>
    <TriangleAlert size={13} aria-hidden="true" />
    {problem.text}
  </button>
{/if}

<style>
  .problem {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    padding: 0 6px;
    border: 0;
    border-radius: var(--radius-control);
    background: transparent;
    color: var(--git-modified-text);
    font: inherit;
    cursor: pointer;
  }
  .problem:hover {
    background: var(--hover);
  }
</style>

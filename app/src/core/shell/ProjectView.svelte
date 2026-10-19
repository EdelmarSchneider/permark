<script lang="ts">
  // Project document area (placeholder until the reading view exists): what is open, or a quiet prompt.
  import type { WorkspaceView } from "../../api";
  import { t } from "../i18n";

  type Props = { workspace: Extract<WorkspaceView, { kind: "project" }>; document: string | null };
  let { workspace, document }: Props = $props();
</script>

<div class="placeholder">
  {#if document}
    <p>{t("project.document", { path: document })}</p>
  {:else}
    <p class="quiet">{t("project.noDocument")}</p>
  {/if}
  <p class="root" title={workspace.root}>{workspace.root}</p>
</div>

<style>
  .placeholder {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: var(--sp-2);
    height: 100%;
    padding: var(--sp-6);
    text-align: center;
  }
  p {
    margin: 0;
    max-width: var(--measure);
  }
  .quiet {
    color: var(--muted);
  }
  .root {
    color: var(--faint);
    font-size: var(--fs-ui-sm);
    overflow-wrap: anywhere;
  }
</style>

<script lang="ts">
  // Single file (placeholder until the reading view exists): the file and where it lives.
  import type { WorkspaceView } from "../../api";
  import { t } from "../i18n";

  type Props = { workspace: Extract<WorkspaceView, { kind: "singleFile" }> };
  let { workspace }: Props = $props();
</script>

<div class="placeholder">
  <p class="name">{workspace.name}</p>
  <p class="folder" title={workspace.folder}>{t("file.folder", { folder: workspace.folder })}</p>
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
  .name {
    font: 600 var(--fs-reading) / var(--lh-reading) var(--font-reading);
  }
  .folder {
    color: var(--faint);
    font-size: var(--fs-ui-sm);
    overflow-wrap: anywhere;
  }
</style>

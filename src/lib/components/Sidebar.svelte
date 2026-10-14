<script lang="ts">
  import { t } from "../i18n";
  import { session } from "../stores/session.svelte";
  import { ui } from "../stores/ui.svelte";
  import Explorer from "./Explorer.svelte";
  import Resizer from "./Resizer.svelte";
  import SearchPanel from "./SearchPanel.svelte";
  import TagsPanel from "./TagsPanel.svelte";
</script>

<aside class="sidebar" aria-label={t("sidebar.label")} inert={!session.left.open}>
  <Resizer side="left" />
  <!-- the inner column keeps its width while the panel animates, so the content does not reflow -->
  <div class="panel-inner" style:width="{session.left.width}px">
    <section class="panel-inner" hidden={ui.sidebarView !== "explorer"}>
      <Explorer />
    </section>
    <section class="panel-inner" hidden={ui.sidebarView !== "search"}>
      <SearchPanel />
    </section>
    <section class="panel-inner" hidden={ui.sidebarView !== "tags"}>
      <TagsPanel />
    </section>
  </div>
</aside>

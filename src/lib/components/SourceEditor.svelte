<script lang="ts">
  // The Source editor in the document pane. CodeMirror is loaded on first use, so reading never pays for it.
  import { onMount } from "svelte";
  import { bridge } from "../bridge";
  import type { SourceView } from "../editor/cm";
  import { t } from "../i18n";
  import { basename } from "../paths";
  import { edit } from "../stores/edit.svelte";
  import Notice from "./Notice.svelte";

  let host: HTMLElement | undefined = $state();
  let ready = $state(false);
  let failed = $state(false);

  onMount(() => {
    const start = edit.initial;
    const mount = host;
    if (!mount || !start) return;
    let view: SourceView | null = null;
    let gone = false;
    import("../editor/cm")
      .then(({ createSourceView }) => {
        if (gone) return;
        view = createSourceView(
          mount,
          start.buffer,
          start.line,
          t("edit.sourceLabel", { name: basename(edit.path ?? "") }),
          {
            onChange: (length) => edit.noteChange(length),
            onCaret: (line, column) => edit.noteCaret(line, column),
            onFiles: (files, source) => void edit.addFiles(files, source),
          },
          start.atEnd === true,
        );
        bridge.editor = view;
        ready = true;
        view.focus();
      })
      .catch(() => {
        failed = true;
      });
    return () => {
      gone = true;
      if (view && bridge.editor === view) bridge.editor = undefined;
      view?.destroy();
    };
  });
</script>

<div class="source" aria-busy={!ready && !failed}>
  <div bind:this={host} class="source__host"></div>
  {#if failed}
    <Notice icon="circle-alert" title={t("edit.loadFailed")} />
  {:else if !ready}
    <div class="skeleton" aria-hidden="true">
      <span></span><span></span><span></span><span></span>
    </div>
  {/if}
</div>

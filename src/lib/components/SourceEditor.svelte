<script lang="ts">
  // The Source editor in the document pane. CodeMirror is loaded on first use, so reading never pays for it.
  import { onMount } from "svelte";
  import { bridge } from "../bridge";
  import type { SourceView } from "../editor/cm";
  import { mermaidAt, type MermaidFence } from "../editor/mermaidAt";
  import { EditorOutline } from "../editor/outlineSync";
  import { t } from "../i18n";
  import { basename } from "../paths";
  import { edit } from "../stores/edit.svelte";
  import { outline } from "../stores/outline.svelte";
  import DiagramPreview from "./DiagramPreview.svelte";
  import Notice from "./Notice.svelte";

  let host: HTMLElement | undefined = $state();
  let ready = $state(false);
  let failed = $state(false);
  /** The Mermaid block the caret is in, drawn beside the text. */
  let diagram = $state<MermaidFence | null>(null);

  onMount(() => {
    const start = edit.initial;
    const mount = host;
    if (!mount || !start) return;
    let view: SourceView | null = null;
    let sync: EditorOutline | null = null;
    let gone = false;
    import("../editor/cm")
      .then(({ createSourceView }) => {
        if (gone) return;
        const look = (): void => {
          if (!view) return;
          diagram = mermaidAt(view.getText(), view.caretLine() + 1);
        };
        view = createSourceView(
          mount,
          start.buffer,
          start.line,
          t("edit.sourceLabel", { name: basename(edit.path ?? "") }),
          {
            onChange: (length) => {
              edit.noteChange(length);
              sync?.changed();
              look();
            },
            onCaret: (line, column) => {
              edit.noteCaret(line, column);
              look();
            },
            onFiles: (files, source) => void edit.addFiles(files, source),
          },
          start.atEnd === true,
          start.top,
        );
        look();
        bridge.editor = view;
        // the reading view is hidden while editing, so the Outline is built from, and scrolls, this editor
        if (edit.path) {
          sync = new EditorOutline(edit.path, view, outline);
          bridge.editorOutline = sync;
          sync.start();
        }
        ready = true;
        view.focus();
      })
      .catch(() => {
        failed = true;
      });
    return () => {
      gone = true;
      if (view && bridge.editor === view) bridge.editor = undefined;
      if (sync && bridge.editorOutline === sync) bridge.editorOutline = undefined;
      sync?.dispose();
      view?.destroy();
    };
  });
</script>

<div class={["source", diagram && "source--diagram"]} aria-busy={!ready && !failed}>
  <div bind:this={host} class="source__host"></div>
  {#if diagram}
    <DiagramPreview source={diagram.source} fence={diagram.startLine} />
  {/if}
  {#if failed}
    <Notice icon="circle-alert" title={t("edit.loadFailed")} />
  {:else if !ready}
    <div class="skeleton" aria-hidden="true">
      <span></span><span></span><span></span><span></span>
    </div>
  {/if}
</div>

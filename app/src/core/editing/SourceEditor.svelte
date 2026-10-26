<script lang="ts">
  // Source mode: the raw file in CodeMirror (loaded on first use), with its find + replace panel and go to line.
  // Nothing is hidden or drawn over the text. Beside the editor (never over it), an optional panel draws the diagram
  // the caret is in (off by default, one switch per window).
  import { untrack } from "svelte";
  import { SEARCH_PHRASES } from "./source/phrases";
  import { api } from "../../api";
  import { t } from "../i18n";
  import { look } from "../look.svelte";
  import type { Content, EditingSession } from "./session.svelte";
  import type { SourceView } from "./source/cm";
  import type { Fence } from "./source/fence";
  import DiagramPreview from "./DiagramPreview.svelte";
  import { diagramPanel } from "./diagramPanel.svelte";
  import { pictureFiles } from "./pictures";

  type Props = {
    session: EditingSession;
    content: Content;
    markdown: boolean;
    /** Single file: no project files to link to. */
    project: boolean;
    pictures: (files: File[]) => Promise<string[]>;
  };
  let { session, content, markdown, project, pictures }: Props = $props();

  let host: HTMLElement | undefined = $state();
  let source: SourceView | null = null;
  let fence = $state.raw<Fence | null>(null);
  const panel = $derived(markdown && diagramPanel.on);

  // the editor: made when the content (a load, a mode switch) is new, torn down with it
  $effect(() => {
    const target = host;
    const current = content;
    if (!target) return;
    let gone = false;
    let detach = (): void => {};
    void import("./source/cm").then(({ createSource }) => {
      if (gone) return;
      const made = createSource({
        parent: target,
        text: current.text,
        path: current.path,
        markdown,
        wrap: look.wrap,
        phrases: Object.fromEntries(SEARCH_PHRASES.map(([phrase, key]) => [phrase, t(key)])),
        host: {
          changed: () => session.changed(),
          selected: () => session.selected(),
          save: () => session.save(),
          modeKey: (toSource) => void session.switchTo(toSource ? "source" : "view"),
          pictures: (files, at) => void insertPictures(files, at),
          fence: (next) => (fence = next),
          docPath: () => current.path,
          linkTargets: async () => {
            if (!project) return [];
            const result = await api.editLinkTargets();
            return result.ok ? result.value : [];
          },
          headings: async (path) => {
            const result = await api.editHeadings(path);
            return result.ok ? result.value.map((h) => ({ id: h.id, label: h.label })) : [];
          },
        },
      });
      source = made;
      made.scrollToLine(current.line);
      made.view.focus();
      detach = session.attach(current.seq, {
        kind: "source",
        text: () => made.text(),
        changes: (base) => changedRuns(base, made.text()),
        topLine: () => made.topLine(),
        selection: () => made.selection(),
        focus: () => made.view.focus(),
        pictures: (files, point) => {
          const at = made.view.posAtCoords(point) ?? made.view.state.selection.main.head;
          void insertPictures(files, at);
        },
      });
    });
    return () => {
      gone = true;
      detach();
      source?.destroy();
      source = null;
      fence = null;
    };
  });

  // the wrap setting changed: the open editor follows (place and caret stay)
  $effect(() => {
    const on = look.wrap;
    untrack(() => source?.setWrap(on));
  });

  /** Runs of changed lines (save message). */
  function changedRuns(base: string, text: string): number {
    const a = base.split("\n");
    const b = text.split("\n");
    let runs = 0;
    let inRun = false;
    const n = Math.max(a.length, b.length);
    for (let i = 0; i < n; i++) {
      const differs = a[i] !== b[i];
      if (differs && !inRun) runs += 1;
      inRun = differs;
    }
    return runs;
  }

  async function insertPictures(files: File[], at: number): Promise<void> {
    const paths = await pictures(pictureFiles(files));
    if (paths.length === 0 || !source) return;
    source.insert(paths.map((p) => `![](${p})`).join("\n"), at);
  }
</script>

<div class="source" data-drop-editor>
  <div class="body">
    <div class="cm" bind:this={host}></div>
    {#if panel}
      <aside class="preview" aria-label={t("edit.diagramPanel")}>
        {#if fence}
          <DiagramPreview language={fence.language} source={fence.code} />
        {:else}
          <p class="empty">{t("edit.diagramPanelEmpty")}</p>
        {/if}
      </aside>
    {/if}
  </div>
</div>

<style>
  .source {
    display: flex;
    flex-direction: column;
    min-height: 100%;
  }
  .body {
    display: flex;
    flex: 1;
    min-height: 0;
  }
  .cm {
    flex: 1;
    min-width: 0;
  }
  .cm :global(.cm-editor) {
    min-height: 100%;
  }
  /* beside the text, never over it */
  .preview {
    flex: 0 0 40%;
    max-width: 520px;
    border-left: 1px solid var(--border);
    padding: var(--sp-3);
    overflow: auto;
    position: sticky;
    top: 0;
    max-height: 100vh;
  }
  .empty {
    color: var(--muted);
    font-size: var(--fs-ui-sm);
  }
</style>

<script lang="ts">
  // Source mode: the raw file in CodeMirror (loaded on first use), with its find + replace panel and go to line.
  // Nothing is hidden, masked or drawn over the text: secret values show as they are, diagrams draw in View and Edit.
  import { SEARCH_PHRASES } from "./source/phrases";
  import { api } from "../../api";
  import { t } from "../i18n";
  import type { Content, EditingSession } from "./session.svelte";
  import type { SourceView } from "./source/cm";
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
        phrases: Object.fromEntries(SEARCH_PHRASES.map(([phrase, key]) => [phrase, t(key)])),
        host: {
          changed: () => session.changed(),
          selected: () => session.selected(),
          save: () => session.save(),
          modeKey: (toSource) => void session.switchTo(toSource ? "source" : "view"),
          pictures: (files, at) => void insertPictures(files, at),
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
    };
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
</style>

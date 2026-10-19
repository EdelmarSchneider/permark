<script lang="ts">
  // Source mode: CodeMirror over the edit text (loaded on first use), a small find bar, the diagram drawn beside
  // the fence the caret is in.
  import { api } from "../../api";
  import { t } from "../i18n";
  import type { Content, EditingSession } from "./session.svelte";
  import type { Fence, SourceView } from "./source/cm";
  import DiagramPreview from "./DiagramPreview.svelte";
  import { pictureFiles } from "./pictures";

  type Props = {
    session: EditingSession;
    content: Content;
    markdown: boolean;
    /** A config file (a sheet format): its secret values are masked. */
    config?: boolean;
    /** Single file: no project files to link to. */
    project: boolean;
    pictures: (files: File[]) => Promise<string[]>;
  };
  let { session, content, markdown, config = false, project, pictures }: Props = $props();

  let host: HTMLElement | undefined = $state();
  let finding = $state(false);
  let query = $state("");
  let missing = $state(false);
  let fence = $state.raw<Fence | null>(null);
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
        secrets: config
          ? {
              load: async (text) => {
                const result = await api.configSecrets(current.path, text);
                return result.ok ? result.value : [];
              },
              label: t("config.reveal"),
            }
          : undefined,
        host: {
          changed: () => session.changed(),
          selected: () => session.selected(),
          save: () => session.save(),
          modeKey: (toSource) => void session.switchTo(toSource ? "source" : "view"),
          find: () => (finding = true),
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

  function find(backwards = false): void {
    missing = source ? !source.findNext(query, backwards) : true;
  }

  function onFindKey(event: KeyboardEvent): void {
    if (event.isComposing) return;
    if (event.key === "Enter") {
      event.preventDefault();
      find(event.shiftKey);
    } else if (event.key === "Escape") {
      event.preventDefault();
      finding = false;
      source?.view.focus();
    }
  }
</script>

<div class="source" class:with-preview={fence !== null} data-drop-editor>
  {#if finding}
    <div class="find" role="search">
      <input
        class="field"
        type="search"
        placeholder={t("find.placeholder")}
        aria-label={t("find.placeholder")}
        bind:value={query}
        onkeydown={onFindKey}
        oninput={() => (missing = false)}
      />
      <button type="button" class="btn btn--sm" onclick={() => find(true)}
        >{t("find.previous")}</button
      >
      <button type="button" class="btn btn--sm" onclick={() => find(false)}>{t("find.next")}</button
      >
      {#if missing}<span class="missing" role="status">{t("find.none")}</span>{/if}
      <button type="button" class="btn btn--sm btn--ghost" onclick={() => (finding = false)}>
        {t("common.close")}
      </button>
    </div>
  {/if}
  <div class="body">
    <div class="cm" bind:this={host}></div>
    {#if fence}
      <aside class="preview" aria-label={t("edit.diagramPreview")}>
        <DiagramPreview language={fence.language} source={fence.code} />
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
  .find {
    position: sticky;
    top: 0;
    z-index: 4;
    display: flex;
    align-items: center;
    gap: var(--sp-2);
    padding: var(--sp-2) var(--sp-3);
    background: var(--surface);
    border-bottom: 1px solid var(--border);
  }
  .find .field {
    flex: 1;
    min-width: 0;
    max-width: 320px;
  }
  .missing {
    color: var(--muted);
    font-size: var(--fs-ui-sm);
  }
</style>

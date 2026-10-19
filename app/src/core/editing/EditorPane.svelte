<script lang="ts">
  // The document area while editing: the editor of the mode (Edit | Source), the conflict banner, a failed save,
  // the unsaved prompt, and the question before a large picture is saved.
  import { api } from "../../api";
  import { apiErrorText } from "../errors";
  import { t } from "../i18n";
  import ConflictView from "./ConflictView.svelte";
  import { ASK_ABOVE, REFUSE_ABOVE, extensionOf } from "./pictures";
  import type { EditingSession } from "./session.svelte";
  import SourceEditor from "./SourceEditor.svelte";
  import VisualEditor from "./VisualEditor.svelte";

  type Props = {
    session: EditingSession;
    scroller: HTMLElement;
    project: boolean;
    note: (text: string) => void;
  };
  let { session, scroller, project, note }: Props = $props();

  const doc = $derived(session.state);
  const content = $derived(session.content);
  const conflict = $derived(
    doc.s === "conflict"
      ? doc
      : doc.s === "unsavedPrompt" && doc.back.s === "conflict"
        ? doc.back
        : null,
  );

  type Ask =
    { kind: "none" } | { kind: "big"; name: string; size: string; answer: (ok: boolean) => void };
  let ask = $state.raw<Ask>({ kind: "none" });

  function megabytes(bytes: number): string {
    return (bytes / (1024 * 1024)).toFixed(1);
  }

  function askBig(file: File): Promise<boolean> {
    return new Promise((resolve) => {
      ask = {
        kind: "big",
        name: file.name,
        size: megabytes(file.size),
        answer: (ok) => {
          ask = { kind: "none" };
          resolve(ok);
        },
      };
    });
  }

  /** Saves pictures next to the document; resolves to the saved paths. */
  async function pictures(files: File[]): Promise<string[]> {
    const doc = session.path;
    if (doc === null) return [];
    const saved: string[] = [];
    for (const file of files) {
      const ext = extensionOf(file);
      if (ext === null) continue;
      if (file.size > REFUSE_ABOVE) {
        note(t("images.tooLarge", { name: file.name, limit: 50 }));
        continue;
      }
      if (file.size > ASK_ABOVE && !(await askBig(file))) continue;
      const bytes = new Uint8Array(await file.arrayBuffer());
      const result = await api.editWriteAsset(doc, ext, bytes);
      if (result.ok) saved.push(result.value);
      else note(apiErrorText(result.error));
    }
    return saved;
  }

  async function copyAll(): Promise<void> {
    try {
      await navigator.clipboard.writeText(session.text);
      note(t("edit.copied"));
    } catch {
      note(t("doc.copyFailed"));
    }
  }
</script>

<div class="pane">
  {#if conflict}
    <ConflictView
      theirs={session.disk?.text ?? null}
      deleted={conflict.disk === null}
      mine={session.text}
      reload={() => session.dispatch({ e: "reloadTheirs" })}
      keepMine={() => session.dispatch({ e: "keepMine" })}
      saveAsNew={() => session.dispatch({ e: "saveAsNew" })}
    />
  {/if}
  {#if doc.s === "saveError"}
    <div class="error" role="alert">
      <span>{t("edit.saveError")}</span>
      <button
        type="button"
        class="btn btn--sm btn--primary"
        onclick={() => session.dispatch({ e: "retry" })}
      >
        {t("edit.retry")}
      </button>
      <button type="button" class="btn btn--sm" onclick={() => void copyAll()}
        >{t("edit.copyAll")}</button
      >
    </div>
  {/if}
  {#if ask.kind === "big"}
    {@const current = ask}
    <div class="error" role="alertdialog" aria-label={t("images.bigTitle")}>
      <span>{t("images.big", { name: current.name, size: current.size })}</span>
      <button type="button" class="btn btn--sm btn--primary" onclick={() => current.answer(true)}>
        {t("images.saveAnyway")}
      </button>
      <button type="button" class="btn btn--sm" onclick={() => current.answer(false)}
        >{t("common.cancel")}</button
      >
    </div>
  {/if}

  {#if doc.s === "loading" || content === null}
    <div class="loading" role="progressbar" aria-label={t("doc.loading")}></div>
  {:else if session.mode === "edit"}
    {#key content.seq}
      <VisualEditor
        {session}
        {content}
        {scroller}
        {project}
        {pictures}
        failed={(error) => {
          note(t("edit.visualFailed"));
          console.error(error);
          void session.switchTo("source");
        }}
      />
    {/key}
  {:else if session.mode === "source"}
    {#key content.seq}
      <SourceEditor {session} {content} markdown={session.markdown} {project} {pictures} />
    {/key}
  {/if}
</div>

{#if doc.s === "unsavedPrompt"}
  <div class="backdrop">
    <div class="dialog" role="dialog" aria-modal="true" aria-labelledby="pm-unsaved-title">
      <h2 id="pm-unsaved-title">{t("edit.unsavedTitle")}</h2>
      <p>{t("edit.unsavedText", { name: session.path?.split("/").at(-1) ?? "" })}</p>
      <div class="buttons">
        {#if doc.back.s !== "conflict"}
          <button type="button" class="btn btn--primary" onclick={() => session.answer("save")}>
            {t("edit.save")}
          </button>
        {/if}
        <button type="button" class="btn" onclick={() => session.answer("discard")}
          >{t("edit.discard")}</button
        >
        <!-- svelte-ignore a11y_autofocus -->
        <button type="button" class="btn" autofocus onclick={() => session.answer("keepEditing")}>
          {t("edit.keepEditing")}
        </button>
      </div>
    </div>
  </div>
{/if}

<style>
  .pane {
    display: flex;
    flex-direction: column;
    min-height: 100%;
  }
  .error {
    position: sticky;
    top: 0;
    z-index: 7;
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--sp-2);
    padding: var(--sp-2) var(--sp-4);
    border-bottom: 1px solid var(--border);
    background: color-mix(in srgb, var(--git-deleted) 8%, var(--bg));
    font-size: var(--fs-ui);
  }
  .error span {
    flex: 1 1 240px;
  }
  .loading {
    height: 2px;
    background: var(--accent);
    opacity: 0.6;
  }
  .backdrop {
    position: fixed;
    inset: 0;
    z-index: 50;
    display: grid;
    place-items: center;
    padding: var(--sp-4);
    background: var(--backdrop);
  }
  .dialog {
    width: min(440px, 100%);
    padding: var(--sp-5);
    border-radius: var(--radius-card);
    background: var(--bg);
    box-shadow: var(--shadow-pop);
  }
  .dialog h2 {
    margin: 0 0 var(--sp-2);
    font-size: 15px;
  }
  .dialog p {
    margin: 0 0 var(--sp-4);
    color: var(--muted);
    overflow-wrap: anywhere;
  }
  .buttons {
    display: flex;
    flex-wrap: wrap;
    justify-content: flex-end;
    gap: var(--sp-2);
  }
</style>

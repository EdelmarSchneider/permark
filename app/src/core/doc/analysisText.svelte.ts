// The text an analysis (word count, token count) measures: the document the window shows, or the buffer being edited,
// and the selection. Call `createAnalysisText()` while a component starts: it ties the reading to that component.
// In View the file's text is read once per version (several analyses share the read); while editing the buffer is used.

import { api } from "../../api";
import { editing } from "../editing/session.svelte";
import { documents } from "./document.svelte";

type Loaded = { path: string; hash: string | null; text: string };

/** The latest text read per window: the second analysis asking for the same version finds it here. */
let latest: Loaded | null = null;
const reading = new Map<string, Promise<void>>();

function readOnce(path: string, hash: string | null): Promise<void> {
  if (latest?.path === path && latest.hash === hash) return Promise.resolve();
  const key = `${path}\u0000${hash ?? ""}`;
  const pending = reading.get(key);
  if (pending) return pending;
  const job = api.documentText(path).then((result) => {
    if (result.ok) latest = { path, hash, text: result.value };
    reading.delete(key);
  });
  reading.set(key, job);
  return job;
}

export type AnalysisText = {
  /** A document with text is shown (Markdown or text). */
  readonly countable: boolean;
  /** The document is Markdown (syntax is not part of what is measured). */
  readonly markdown: boolean;
  /** The text of the document: the buffer while editing, else the file. "" until it is read. */
  readonly text: string;
  /** The selected text ("" = none). */
  readonly selection: string;
  /** Selected text is Markdown only in the Source editor; in View and Edit it is what the page shows. */
  readonly selectionMarkdown: boolean;
  /** The document path, when one is shown. */
  readonly path: string | null;
  /** The text is a buffer being edited (it changes with the keys), not the file. */
  readonly editing: boolean;
};

export function createAnalysisText(): AnalysisText {
  let viewText = $state.raw<Loaded | null>(null);
  let viewSelection = $state("");

  const view = $derived(documents.view);
  const editingNow = $derived(editing.mode !== "view" && editing.path === documents.path);
  const markdown = $derived(view?.format === "markdown");
  const countable = $derived(
    view !== null && (view.body.kind === "markdown" || view.body.kind === "text"),
  );

  // the file's text in View: read once per version (external I/O)
  $effect(() => {
    const current = view;
    if (!current || !countable || editingNow) return;
    if (viewText?.path === current.path && viewText.hash === current.hash) return;
    let gone = false;
    void readOnce(current.path, current.hash).then(() => {
      if (!gone && latest?.path === current.path && latest.hash === current.hash) viewText = latest;
    });
    return () => {
      gone = true;
    };
  });

  // the selection of the page in View
  $effect(() => {
    if (editingNow) return;
    const onSelection = (): void => {
      const selection = window.getSelection();
      const node = selection?.anchorNode ?? null;
      const inPage = node !== null && node.parentElement?.closest("[data-scroller]") !== null;
      viewSelection = inPage ? (selection?.toString() ?? "") : "";
    };
    document.addEventListener("selectionchange", onSelection);
    return () => document.removeEventListener("selectionchange", onSelection);
  });

  return {
    get countable() {
      return countable;
    },
    get markdown() {
      return markdown;
    },
    get text() {
      return editingNow
        ? editing.text
        : viewText?.path === view?.path
          ? (viewText?.text ?? "")
          : "";
    },
    get selection() {
      return editingNow ? editing.selection : viewSelection;
    },
    get selectionMarkdown() {
      return editingNow && editing.mode === "source";
    },
    get path() {
      return view?.path ?? null;
    },
    get editing() {
      return editingNow;
    },
  };
}

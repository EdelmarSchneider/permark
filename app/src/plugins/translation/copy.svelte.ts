// A document translated into a new file beside it. The whole text as the editor shows it when asked (unsaved edits
// included) goes to `translation.run` piece by piece, cut as the review cuts it (`local-ai/pieces/segment.ts`), under
// a run id of its own; each answer is held to the review's checks (`guards.ts judge`). Nothing is shown for review
// and nothing in the editor changes: when the run is done the translated parts are put into a copy of the text in
// memory (every other byte as it was), written ONCE as a new file (`translation.save`: never over a file, a taken
// name gets a number; the line ending of the original), and that file opens. Parts with no usable answer stay in the
// original language, counted for the person; nothing translated at all -> no file. Stopped | failed | the plugin off
// -> the run is stopped and nothing is written. Once started it needs the editor no more: the person may edit on,
// change mode or document. Not the workspace: the backend writes into the one the window shows, so a window that
// shows another one by then stops the copy instead of writing there. The plugin's backend takes one run at a time:
// the entries (`index.ts`) keep this run and the review's apart.

import { api as defaultApi, type Api } from "../../api";
import type { PieceRun } from "../../api/generated/PieceRun";
import type { PieceRunResult } from "../../api/generated/PieceRunResult";
import { documents } from "../../core/doc/document.svelte";
import { editing, type EditingSession, type TextRange } from "../../core/editing/session.svelte";
import { apiErrorText } from "../../core/errors";
import { nameOf, stemLength } from "../../core/explorer/list";
import { language, t, tn } from "../../core/i18n";
import { ui } from "../../core/shell/ui.svelte";
import { windowStore, type NoticeAction } from "../../core/window.svelte";
import {
  PieceRunLink,
  type RunEnd,
  type RunPreparing,
  type RunStatus,
} from "../local-ai/pieces/review.svelte";
import { pieces, type Piece } from "../local-ai/pieces/segment";
import { LOCAL_AI_PAGE } from "../local-ai/store.svelte";
import { runErrorText } from "../local-ai/words";
import { judge } from "./guards";
import { LANGUAGES, namesIn } from "./languages";
import { translation, type Languages, type RunLanguages } from "./store.svelte";

/** A part of a file name shaped like a language: its code, then a script and | or a region at most (`pt`, `pt-BR`,
 *  `zh-Hans`, `sr-Latn-RS`, `es-419`). */
const LANGUAGE_PART = /^([a-z]{2,3})(?:-[a-z]{4})?(?:-(?:[a-z]{2}|\d{3}))?$/i;

/** Codes of the languages offered, without their variants (`en` of `en-US`), lowercase. Built on the first name
 *  asked for. */
let codes: ReadonlySet<string> | null = null;

/** `part` names a language of the list, in whatever variant and letter case (`en`, `PT-br`, `zh-CN`). */
function namesLanguage(part: string): boolean {
  const code = LANGUAGE_PART.exec(part)?.[1]?.toLowerCase();
  if (code === undefined) return false;
  codes ??= new Set(LANGUAGES.map((tag) => (tag.split("-")[0] ?? tag).toLowerCase()));
  return codes.has(code);
}

/** The name of the copy of file `name` in the language `tag`: the tag before the extension (`readme.md` ->
 *  `readme.pt-BR.md`; a name without one: at its end), in the place of the language the name carries there already
 *  (`readme.en.md` -> `readme.pt-BR.md`). */
export function copyName(name: string, tag: string): string {
  const cut = stemLength(name);
  const stem = name.slice(0, cut);
  const dot = stem.lastIndexOf(".");
  const base = dot > 0 && namesLanguage(stem.slice(dot + 1)) ? stem.slice(0, dot) : stem;
  return `${base}.${tag}${name.slice(cut)}`;
}

/** What the copy needs of the editing session: the document being edited and its text now. */
export type CopySession = Pick<EditingSession, "path" | "current">;

/** What the copy asks of the window. */
export type CopyWindow = {
  /** The workspace it shows (`null`: none): a file is written into the one shown when it is written. */
  workspace(): number | null;
  /** Opens a document. */
  open(path: string): void;
  /** A quiet line that goes by itself. */
  note(text: string): void;
  /** A line (with buttons) that stays until the person dismisses it: the end of a run they may not be watching. */
  lasting(text: string, ...actions: NoticeAction[]): void;
};

const WINDOW: CopyWindow = {
  workspace: () => documents.workspaceId,
  open: (path) => documents.navigate(path, null),
  note: (text) => void windowStore.notifyNote(text),
  lasting: (text, ...actions) =>
    void windowStore.notifyLasting({ text, actions, onGone: () => {} }),
};

export type CopyState =
  | { kind: "idle" }
  | {
      kind: "running";
      /** Name of the file being translated. */
      file: string;
      /** Where the run stands: getting ready, then the part the model is on. */
      status: RunPreparing | Extract<RunStatus, { kind: "checking" }>;
    };

/** A copy being made. */
type Job = {
  /** The workspace the document is in: the copy is written only while the window still shows it. */
  workspace: number | null;
  /** The document translated: the new file goes beside it. */
  path: string;
  /** Its text when the run started (LF); the pieces' offsets are in it. */
  text: string;
  /** The languages the run was sent: its answers are judged against them. */
  sent: RunLanguages;
  /** Pieces sent and not answered yet, by id. */
  waiting: Map<number, Piece>;
  parts: number;
  answered: number;
  /** What goes where in `text`: one range per translated part. */
  translated: (TextRange & { insert: string })[];
  /** Parts whose answer was no translation to trust. */
  failed: number;
  /** The original ends its lines with CR LF on disk. */
  crlf: Promise<boolean>;
};

const IDLE: CopyState = { kind: "idle" };

export class TranslationCopy {
  state = $state.raw<CopyState>(IDLE);

  readonly #languages: Languages;
  readonly #api: Api;
  readonly #session: CopySession;
  readonly #window: CopyWindow;
  /** The run in the plugin's backend. */
  readonly #runs: PieceRunLink;
  /** The copy being made (`null`: none). */
  #job: Job | null = null;

  constructor(
    languages: Languages = translation.languages,
    api: Api = defaultApi,
    session: CopySession = editing,
    shown: CopyWindow = WINDOW,
  ) {
    this.#languages = languages;
    this.#api = api;
    this.#session = session;
    this.#window = shown;
    this.#runs = new PieceRunLink("translation", api, {
      preparing: (status) => this.#show(status),
      answer: (id, result) => this.#answer(id, result),
      end: (status) => void this.#end(status),
    });
  }

  /** A copy is being made. */
  get running(): boolean {
    return this.state.kind === "running";
  }

  /** Hears the run news until the returned teardown runs; the teardown stops a copy on its way: nothing is written,
   *  nothing said. */
  listen(): () => void {
    const stopNews = this.#runs.listen();
    return () => {
      stopNews();
      this.#runs.stop();
      this.#clear();
    };
  }

  /** Starts translating the document being edited, as its text stands now, into the language chosen. Nothing while
   *  a copy is on its way, or with no language | no document. */
  async run(): Promise<void> {
    if (this.#job !== null) return;
    const sent = this.#languages.runArgs();
    const path = this.#session.path;
    if (sent === null || path === null) return;
    const text = this.#session.current();
    const cut = pieces(text);
    if (cut.length === 0) {
      this.#window.note(t("translation.nothing"));
      return;
    }
    this.#job = {
      workspace: this.#window.workspace(),
      path,
      text,
      sent,
      waiting: new Map(cut.map((piece, id) => [id, piece])),
      parts: cut.length,
      answered: 0,
      translated: [],
      failed: 0,
      crlf: this.#crlf(path),
    };
    this.state = {
      kind: "running",
      file: nameOf(path),
      status: { kind: "loading", progress: 0 },
    };
    await this.#runs.start(
      cut.map((piece, id) => ({ id, text: piece.text })),
      sent,
    );
  }

  /** News of the plugin's runs: only the copy's own run counts. */
  news(news: PieceRun): void {
    this.#runs.news(news);
  }

  /** Stops the copy: nothing is written. Too late once the run is over (the file is being written). */
  cancel(): void {
    if (!this.#runs.stop()) return;
    this.#clear();
    this.#window.note(stoppedText());
  }

  #show(status: Extract<CopyState, { kind: "running" }>["status"]): void {
    if (this.state.kind === "running") this.state = { ...this.state, status };
  }

  /** The window shows another workspace than the one `job` started in: its file would be written there. The copy
   *  is let go, and the person told. */
  #strayed(job: Job): boolean {
    if (this.#window.workspace() === job.workspace) return false;
    this.#runs.stop();
    this.#clear();
    this.#window.lasting(stoppedText());
    return true;
  }

  #answer(id: number, result: PieceRunResult): void {
    const job = this.#job;
    if (job === null || this.#strayed(job)) return;
    job.answered += 1;
    this.#show({
      kind: "checking",
      part: Math.min(job.answered + 1, job.parts),
      parts: job.parts,
    });
    const piece = job.waiting.get(id);
    job.waiting.delete(id);
    if (piece === undefined) return;
    if (result.kind === "discarded") {
      job.failed += 1;
      return;
    }
    const verdict = judge(job.text, piece, result.text, {
      target: job.sent.target.tag,
      source: job.sent.source?.tag ?? null,
    });
    switch (verdict.kind) {
      case "ok":
        job.translated.push(...verdict.suggestions);
        return;
      case "discard":
        job.failed += 1;
        return;
      case "same":
        // already in the language: stays as it is, nothing to tell
        return;
      default:
        return verdict satisfies never;
    }
  }

  async #end(status: RunEnd): Promise<void> {
    const job = this.#job;
    if (job === null) return;
    switch (status.kind) {
      case "finished":
        if (!this.#strayed(job)) await this.#finish(job);
        return;
      case "cancelled":
        // not the person's Cancel (that one tells itself): the backend let the run go
        this.#clear();
        this.#window.lasting(stoppedText());
        return;
      case "failed":
        this.#clear();
        this.#window.lasting(
          `${t("translation.failed")} ${runErrorText(status.error, "translation.failed.noModel")}`,
          {
            label: t("translation.openSettings"),
            run: () => ui.openSettingsPage(LOCAL_AI_PAGE, "content"),
          },
        );
        return;
      default:
        return status satisfies never;
    }
  }

  /** The run is done: the translated text is written as the new file, which opens; no part translated: no file. */
  async #finish(job: Job): Promise<void> {
    const into = namesIn(language.locale, "sentence")(job.sent.target.tag);
    // a part the run never answered is no translation either
    const failed = job.failed + job.waiting.size;
    if (job.translated.length === 0) {
      this.#clear();
      const why =
        failed > 0 ? t("translation.failed") : t("translation.copy.same", { language: into });
      this.#window.lasting(`${why} ${t("translation.copy.noFile")}`);
      return;
    }
    const crlf = await job.crlf;
    // let go meanwhile (the plugin went off)
    if (this.#job !== job) return;
    let text = job.text;
    // back to front: a range put in never moves the ones before it
    for (const { from, to, insert } of [...job.translated].sort((a, b) => b.from - a.from))
      text = text.slice(0, from) + insert + text.slice(to);
    const saved = await this.#api.pluginCall("translation.save", {
      beside: job.path,
      name: copyName(nameOf(job.path), job.sent.target.tag),
      text: crlf ? text.replace(/\n/g, "\r\n") : text,
    });
    if (this.#job !== job) return;
    this.#clear();
    if (!saved.ok) {
      this.#window.lasting(`${t("translation.copy.notSaved")} ${apiErrorText(saved.error)}`);
      return;
    }
    this.#window.open(saved.value.path);
    // the name the file got: a taken one was not written over
    const done = t("translation.copy.done", { language: into, name: nameOf(saved.value.path) });
    if (failed > 0) this.#window.lasting(`${done} ${tn("translation.copy.failedParts", failed)}`);
    else this.#window.note(done);
  }

  /** The file of `path` ends its lines with CR LF (a file that cannot be read, or mixes its line endings: no). */
  async #crlf(path: string): Promise<boolean> {
    const source = await this.#api.editSource(path);
    return source.ok && source.value.lineEnding === "crLf";
  }

  #clear(): void {
    this.#job = null;
    this.state = IDLE;
  }
}

function stoppedText(): string {
  return `${t("translation.copy.stopped")} ${t("translation.copy.noFile")}`;
}

/** The copy of this window. */
export const copy = new TranslationCopy();

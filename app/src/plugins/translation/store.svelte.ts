// Translation review of one window: the piece review of `local-ai` (runs, news, suggestions in the text, accept |
// reject, marks: `local-ai/pieces/review.svelte.ts`) set up as a translation, with the languages it runs between.
// Every prose piece of the scope goes to `translation.run` as `local-ai/pieces/segment.ts` cuts it, with the target
// language (and the source when the person named it); each answer -> markers back + guards (`guards.ts`) -> one
// suggestion for the whole piece. Languages: the target chosen last and the recent ones are the backend's
// (`translation.prefs` | `setPrefs`); the source is this window's, until changed. No run starts without a target.

import { api as defaultApi, type Api, type PluginMethods } from "../../api";
import type { TranslationLanguage } from "../../api/generated/TranslationLanguage";
import type { TranslationPrefs } from "../../api/generated/TranslationPrefs";
import { editing, type EditScope } from "../../core/editing/session.svelte";
import { PieceReview, type Session } from "../local-ai/pieces/review.svelte";
import { pieces } from "../local-ai/pieces/segment";
import { judge } from "./guards";
import { englishName, sameTag } from "./languages";

/** What a run is sent beside its id + pieces: each language as its tag + the English name the prompt shows. */
export type RunLanguages = Omit<PluginMethods["translation.run"]["args"], "run" | "pieces">;

function named(tag: string | null): TranslationLanguage | null {
  const name = tag === null ? null : englishName(tag);
  return tag === null || name === null ? null : { tag, name };
}

/** The languages a window translates between. Only languages the prompt can name are taken (`englishName`). */
export class Languages {
  /** Tag of the language translated into (`null`: none chosen yet -> the person is asked before a first run). */
  target = $state<string | null>(null);
  /** Tag of the language translated from (`null`: the model reads it off the text). Never the target. */
  source = $state<string | null>(null);
  /** Targets chosen before, newest first. */
  recent = $state.raw<readonly string[]>([]);
  /** What the last run started in this window was sent (`null`: none yet): its answers are judged against it and its
   *  review is named by it, whatever is chosen since. */
  sent = $state.raw<RunLanguages | null>(null);

  readonly #api: Api;
  /** Calls made to the backend: only the answer of the latest one is taken. */
  #calls = 0;

  constructor(api: Api = defaultApi) {
    this.#api = api;
  }

  /** Reads the target chosen last and the recent ones from the backend. */
  async load(): Promise<void> {
    const call = ++this.#calls;
    const answer = await this.#api.pluginCall("translation.prefs", {});
    if (answer.ok && call === this.#calls) this.#take(answer.value);
  }

  /** `tag` is the target from now on (at once: a run may start right after) and the backend remembers it. */
  async choose(tag: string): Promise<void> {
    if (englishName(tag) === null) return;
    const call = ++this.#calls;
    this.target = tag;
    this.#clearSameSource();
    const answer = await this.#api.pluginCall("translation.setPrefs", { target: tag });
    if (answer.ok && call === this.#calls) this.#take(answer.value);
  }

  /** Names the language the text is in (`null`, or the target itself: the model reads it off the text). */
  from(tag: string | null): void {
    if (tag !== null && englishName(tag) === null) return;
    this.source = tag;
    this.#clearSameSource();
  }

  /** What a run starting now is sent (`null`: no target yet). */
  runArgs(): RunLanguages | null {
    const target = named(this.target);
    return target === null ? null : { target, source: named(this.source) };
  }

  /** Forgets everything (the plugin goes off); an answer still on its way is not taken. */
  reset(): void {
    this.#calls += 1;
    this.target = null;
    this.source = null;
    this.recent = [];
    this.sent = null;
  }

  #take(prefs: TranslationPrefs): void {
    const usable = (tag: string): boolean => englishName(tag) !== null;
    this.target = prefs.target !== null && usable(prefs.target) ? prefs.target : null;
    this.recent = prefs.recent.filter(usable);
    this.#clearSameSource();
  }

  /** A text is not translated into its own language. */
  #clearSameSource(): void {
    if (this.source !== null && this.target !== null && sameTag(this.source, this.target))
      this.source = null;
  }
}

/** The translation review of a window. */
export class Translation extends PieceReview {
  readonly languages: Languages;

  constructor(api: Api = defaultApi, session: Session = editing) {
    const languages = new Languages(api);
    super(
      {
        plugin: "translation",
        prepare: pieces,
        judge: (doc, piece, output) => {
          const sent = languages.sent;
          return sent === null
            ? { kind: "discard", reason: "languages" }
            : judge(doc, piece, output, {
                target: sent.target.tag,
                source: sent.source?.tag ?? null,
              });
        },
        runArgs: () => {
          languages.sent = languages.runArgs();
          return languages.sent ?? {};
        },
      },
      api,
      session,
    );
    this.languages = languages;
  }

  /** Starts a run over `kind` of the editor's text; nothing while no language to translate into is chosen (the
   *  caller asks the person for one first). */
  override runOver(kind: EditScope): Promise<void> {
    return this.languages.runArgs() === null ? Promise.resolve() : super.runOver(kind);
  }
}

/** The review of this window. */
export const translation = new Translation();

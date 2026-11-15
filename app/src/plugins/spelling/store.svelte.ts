// Spelling marks of this window's editor. The open document's words (`tokenize`) are checked by the backend in batches
// of unique words; verdicts are cached per language set, so a pass after typing only sends the words not seen yet. A
// pass runs a moment after the text changes; while the person types, the word at the caret is not marked. Nothing is
// marked while text is composed (IME) or while an AI review is open in the editor (its marks and menu own the text
// then); the marks come back after. The document's languages are asked once per open document (declared, else
// detected from its prose, else the default). No dictionary ready for them: no marks (`needsDownload` says which; a
// download is the person's choice, made elsewhere). The marks live on their own layer: other marks never change.

import { untrack } from "svelte";
import {
  api as defaultApi,
  type Api,
  type AppPluginEventName,
  type AppPluginEvents,
  type SpellingDocLanguage,
  type SpellingLanguage,
} from "../../api";
import { editing, type EditingSession } from "../../core/editing/session.svelte";
import { language as uiLanguage } from "../../core/i18n";
import { pluginClient, type PluginClient } from "../../core/plugins/client";
import { openReview } from "../local-ai/pieces/review.svelte";
import { sampleOf, tokenize, type Token, type Word } from "./tokenize";

/** The editors' mark layer of this plugin. */
export const LAYER = "spelling";
/** Look of the marks (core, `editing/marks.css`). */
const MARK_CLASS = "wavy-mark";
/** Wait after the session saw the text change (its own check waits 150 ms): about 300 ms after the last key. */
export const PAUSE_MS = 150;
/** Most words the backend checks in one call. */
export const BATCH = 5000;

/** What the store needs of the editing session. */
export type SpellingSession = Pick<
  EditingSession,
  | "path"
  | "mode"
  | "text"
  | "current"
  | "apply"
  | "marks"
  | "markAt"
  | "composing"
  | "setNativeSpellcheck"
>;

/** The word menu the person opened on a marked word (client point of the right-click). */
export type MenuAt = { index: number; word: string; x: number; y: number };

export type SpellingDeps = {
  api: Api;
  client: PluginClient;
  session: SpellingSession;
  /** An AI review holds the editor's text (spelling pauses). */
  reviewOpen: () => boolean;
  /** The window's language (the default spelling language follows it until one is picked). */
  uiLocale: () => string;
};

export class SpellingStore {
  /** The dictionaries and where each stands (empty until the backend answered). */
  languages = $state.raw<readonly SpellingLanguage[]>([]);
  /** The open document's languages (`null`: not asked yet | no document). */
  document = $state.raw<{ path: string; language: SpellingDocLanguage } | null>(null);
  /** The words marked now, in document order (index = mark index). */
  misspelled = $state.raw<readonly Word[]>([]);
  /** The word menu asked for (rendered by the plugin's UI); `null`: none. */
  menuAt = $state.raw<MenuAt | null>(null);

  readonly #api: Api;
  readonly #client: PluginClient;
  readonly #session: SpellingSession;
  readonly #reviewOpen: () => boolean;
  readonly #uiLocale: () => string;
  /** Verdicts per language set (`langs` joined): word -> known. */
  readonly #cache = new Map<string, Map<string, boolean>>();
  /** Words the person ignored, per document (this window, until it closes). */
  readonly #ignored = new Map<string, Set<string>>();
  #timer: ReturnType<typeof setTimeout> | null = null;
  /** Bumped by every pass and every stop: an answer for an earlier one is dropped. */
  #seq = 0;
  /** The document the passes are about. */
  #path: string | null = null;
  /** The text the last pass marked (a pass for another text of the same document: the person typed). */
  #checked: string | null = null;
  /** Something is drawn on the layer (an empty list then clears it). */
  #drawn = false;
  #asking: { path: string; answer: Promise<SpellingDocLanguage | null> } | null = null;
  #loading: Promise<void> | null = null;

  constructor(deps: Partial<SpellingDeps> = {}) {
    this.#api = deps.api ?? defaultApi;
    this.#client = deps.client ?? pluginClient;
    this.#session = deps.session ?? editing;
    this.#reviewOpen = deps.reviewOpen ?? (() => openReview() !== null);
    this.#uiLocale = deps.uiLocale ?? (() => uiLanguage.locale);
  }

  /** The document's languages that have no dictionary on this computer yet. */
  get needsDownload(): string[] {
    const doc = this.document;
    if (doc === null || this.languages.length === 0) return [];
    return doc.language.langs.filter((lang) => !this.#ready(lang));
  }

  /** Marks the open document until the returned teardown runs (the webview's own spellcheck off meanwhile). */
  start(): () => void {
    const session = this.#session;
    session.setNativeSpellcheck(false);
    let gone = false;
    const stops: (() => void)[] = [];
    const listen = <E extends AppPluginEventName>(
      event: E,
      handler: (payload: AppPluginEvents[E]) => void,
    ): void => {
      void this.#api.onAppPlugin(event, handler).then(
        (stop) => {
          if (gone) stop();
          else stops.push(stop);
        },
        () => {
          // no backend to listen to (a test, a browser): the passes still run
        },
      );
    };
    listen("spelling:personal", () => this.#forget());
    listen("spelling:download", ({ lang, state }) => {
      this.languages = this.languages.map((l) => (l.id === lang ? { ...l, state } : l));
      if (state.kind === "ready") this.#forget();
    });
    void this.#loadLanguages();
    const stopWatch = $effect.root(() => {
      // the editor's document, mode and text (after the session's change check) and an AI review: outside sources
      $effect(() => {
        const path = session.path;
        const mode = session.mode;
        const text = session.text;
        const paused = this.#reviewOpen();
        untrack(() => this.#follow(path, mode !== "view" && !paused, text));
      });
    });
    return () => {
      gone = true;
      stopWatch();
      for (const stop of stops) stop();
      this.#stop();
      this.#clear();
      session.setNativeSpellcheck(true);
      this.#path = null;
      this.#checked = null;
      this.#asking = null;
      this.#loading = null;
      this.#cache.clear();
      this.document = null;
      this.languages = [];
    };
  }

  /** One pass now over the text on screen (the word at the caret marked too). */
  check(): Promise<void> {
    this.#cancel();
    return this.#pass(false);
  }

  /** Replaces marked word `index` with `text` (one undo step). `false`: nothing changed (composing, the text moved). */
  replace(index: number, text: string): boolean {
    const word = this.misspelled[index];
    if (word === undefined) return false;
    const done = this.#session.apply([
      { from: word.from, to: word.to, insert: text, expect: word.word },
    ]);
    if (done) this.menuAt = null;
    return done;
  }

  /** Accepts `word` in every document from now on (the person's own words, kept in the profile). */
  async addToDictionary(word: string): Promise<boolean> {
    this.menuAt = null;
    const result = await this.#client.call("spelling.personal.add", { word });
    if (result.ok) this.#forget();
    return result.ok;
  }

  /** Leaves `word` unmarked in this document while this window is open. */
  ignore(word: string): void {
    this.menuAt = null;
    const path = this.#session.path;
    if (path === null) return;
    const ignored = this.#ignored.get(path) ?? new Set<string>();
    ignored.add(word);
    this.#ignored.set(path, ignored);
    // the verdicts are cached: the marks change at once
    void this.check();
  }

  /** Corrections for `word` in the document's languages (`[]`: none, or no dictionary answers). */
  async suggestions(word: string): Promise<string[]> {
    const langs = this.document?.language.langs ?? [];
    if (langs.length === 0) return [];
    const result = await this.#client.call("spelling.suggest", { langs, word });
    return result.ok ? result.value : [];
  }

  /** Declares the document's language (`null`: back to detected); the marks follow. `false`: refused. */
  async declare(lang: string | null): Promise<boolean> {
    const path = this.#session.path;
    if (path === null) return false;
    const sample = sampleOf(tokenize(this.#session.current()));
    const result = await this.#client.call("spelling.declare", {
      path,
      lang,
      sample,
      uiLocale: this.#uiLocale(),
    });
    if (!result.ok) return false;
    if (this.#session.path === path) {
      this.document = { path, language: result.value };
      this.#schedule(false, 0);
    }
    return true;
  }

  closeMenu(): void {
    this.menuAt = null;
  }

  // ---- passes --------------------------------------------------------------------------------------------------

  #follow(path: string | null, active: boolean, text: string): void {
    if (path !== this.#path) {
      this.#path = path;
      this.#checked = null;
      this.document = null;
      this.misspelled = [];
      this.menuAt = null;
      // the session dropped the marks of the document it let go
      this.#drawn = false;
    }
    if (path === null || !active) {
      this.#stop();
      this.#clear();
      return;
    }
    this.#schedule(this.#checked !== null && this.#checked !== text);
  }

  #schedule(typing: boolean, delay = PAUSE_MS): void {
    this.#cancel();
    this.#timer = setTimeout(() => {
      this.#timer = null;
      void this.#pass(typing);
    }, delay);
  }

  #cancel(): void {
    if (this.#timer !== null) clearTimeout(this.#timer);
    this.#timer = null;
  }

  /** No pass waits and none in flight counts. */
  #stop(): void {
    this.#cancel();
    this.#seq += 1;
  }

  async #pass(typing: boolean): Promise<void> {
    const seq = ++this.#seq;
    const session = this.#session;
    const path = session.path;
    if (path === null || session.mode === "view" || this.#reviewOpen()) {
      this.#clear();
      return;
    }
    if (session.composing()) {
      this.#schedule(typing);
      return;
    }
    const text = session.current();
    const tokens = tokenize(text);
    const [language] = await Promise.all([this.#languageOf(path, tokens), this.#loadLanguages()]);
    if (seq !== this.#seq) return;
    const langs = language?.langs ?? [];
    if (langs.length === 0 || !langs.some((lang) => this.#ready(lang))) {
      this.#clear();
      return;
    }
    const verdicts = await this.#verdicts(langs, tokens);
    if (seq !== this.#seq) return;
    if (verdicts === null) {
      this.#clear();
      return;
    }
    // the text moved on meanwhile (its change brings the next pass), or a review | composition began
    if (session.path !== path || session.current() !== text) return;
    if (session.composing() || this.#reviewOpen()) return;
    this.#checked = text;
    this.#draw(misspellings(tokens, verdicts, this.#ignored.get(path)), typing);
  }

  /** The document's languages: asked once per open document. */
  async #languageOf(path: string, tokens: readonly Token[]): Promise<SpellingDocLanguage | null> {
    const doc = this.document;
    if (doc?.path === path) return doc.language;
    if (this.#asking?.path !== path) {
      const answer = this.#client
        .call("spelling.language", { path, sample: sampleOf(tokens), uiLocale: this.#uiLocale() })
        .then((result) => (result.ok ? result.value : null));
      this.#asking = { path, answer };
    }
    const language = await this.#asking.answer;
    if (this.#asking?.path === path) this.#asking = null;
    if (language !== null && this.#session.path === path) this.document = { path, language };
    return language;
  }

  #loadLanguages(): Promise<void> {
    this.#loading ??= this.#client.call("spelling.languages", {}).then((result) => {
      if (result.ok) this.languages = result.value;
      else this.#loading = null;
    });
    return this.#loading;
  }

  /** A dictionary for `lang` is on this computer (not known yet: the backend says when asked). */
  #ready(lang: string): boolean {
    if (this.languages.length === 0) return true;
    return this.languages.some((l) => l.id === lang && l.state.kind === "ready");
  }

  /** Verdicts for every word of `tokens` (`null`: the backend refused, no dictionary ready). */
  async #verdicts(
    langs: readonly string[],
    tokens: readonly Token[],
  ): Promise<ReadonlyMap<string, boolean> | null> {
    const key = langs.join("\n");
    const cache = this.#cache.get(key) ?? new Map<string, boolean>();
    this.#cache.set(key, cache);
    const all = new Set(tokens.flatMap((t) => [t.word, ...t.parts.map((p) => p.word)]));
    const unknown = [...all].filter((word) => !cache.has(word));
    for (let at = 0; at < unknown.length; at += BATCH) {
      const words = unknown.slice(at, at + BATCH);
      const result = await this.#client.call("spelling.check", { langs: [...langs], words });
      if (!result.ok) return null;
      words.forEach((word, i) => cache.set(word, result.value[i] ?? true));
    }
    return cache;
  }

  /** Verdicts may have changed (personal words, a dictionary ready): all asked again. */
  #forget(): void {
    this.#cache.clear();
    if (this.#path !== null) this.#schedule(false, 0);
  }

  // ---- marks ---------------------------------------------------------------------------------------------------

  #draw(list: readonly Word[], typing: boolean): void {
    this.#show(list);
    if (!typing) return;
    // the word being typed is not done yet
    const at = this.#session.markAt(LAYER);
    if (at !== null) this.#show(list.filter((_, i) => i !== at));
  }

  #show(list: readonly Word[]): void {
    this.misspelled = list;
    const menu = this.menuAt;
    if (menu !== null && list[menu.index]?.word !== menu.word) this.menuAt = null;
    if (list.length === 0 && !this.#drawn) return;
    this.#drawn = list.length > 0;
    this.#session.marks(list, null, () => {}, LAYER, {
      className: MARK_CLASS,
      menu: (index, x, y) => this.#openMenu(list, index, x, y),
    });
  }

  #openMenu(list: readonly Word[], index: number, x: number, y: number): void {
    const word = list[index];
    if (list !== this.misspelled || word === undefined) return;
    this.menuAt = { index, word: word.word, x, y };
  }

  /** No marks on the layer, no menu. */
  #clear(): void {
    this.menuAt = null;
    this.#show([]);
  }
}

/** The words to mark: unknown, not ignored; of a hyphenated word unknown as a whole, its unknown parts. */
function misspellings(
  tokens: readonly Token[],
  verdicts: ReadonlyMap<string, boolean>,
  ignored: ReadonlySet<string> | undefined,
): Word[] {
  const wrong = (word: string): boolean =>
    verdicts.get(word) === false && ignored?.has(word) !== true;
  const out: Word[] = [];
  for (const token of tokens) {
    if (!wrong(token.word)) continue;
    if (token.parts.length === 0) out.push({ from: token.from, to: token.to, word: token.word });
    else out.push(...token.parts.filter((part) => wrong(part.word)));
  }
  return out;
}

export const spelling = new SpellingStore();

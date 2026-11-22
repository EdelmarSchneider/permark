// Spelling marks of this window's editor. The open document's words (`tokenize`) are checked by the backend in batches
// of unique words; verdicts are cached per language set, so a pass after typing only sends the words not seen yet. A
// pass runs a moment after the text changes; while the person types, the word at the caret is not marked, until the
// caret leaves it. Nothing is marked while text is composed (IME) or while an AI review is open in the editor (its
// marks and menu own the text then); the marks come back after. The document's languages are asked once per open
// document (declared, else detected from its prose, else the default). No dictionary ready for them: no marks
// (`needsDownload` says which; a download starts only from the person's own confirm, `download`). The marks live on
// their own layer: other marks never change. The webview's own spellcheck is off only while a dictionary of the
// document's languages is ready (these marks can show); without one the person keeps it. A large text is read in
// short runs between which the page stays responsive (`tokenizeInSteps`). Also the state of the plugin's menus (a
// marked word's, the language picker, the download question) and of its settings (default language, own words).

import { untrack } from "svelte";
import {
  api as defaultApi,
  type Api,
  type AppPluginEventName,
  type AppPluginEvents,
  type SpellingDefault,
  type SpellingDocLanguage,
  type SpellingFailure,
  type SpellingLanguage,
} from "../../api";
import { isTextTarget } from "../../core/commands/dispatch";
import { editing, type EditingSession } from "../../core/editing/session.svelte";
import { t, language as uiLanguage, type MessageKey } from "../../core/i18n";
import { pluginClient, type PluginClient } from "../../core/plugins/client";
import { arrangement } from "../../core/shell/arrangement.svelte";
import { windowStore, type NoticeAction } from "../../core/window.svelte";
import { openReview } from "../local-ai/pieces/review.svelte";
import { languageLabel } from "./names";
import { sampleOf, tokenizeInSteps, type Token, type Word } from "./tokenize";

/** The editors' mark layer of this plugin. */
export const LAYER = "spelling";
/** Look of the marks (core, `editing/marks.css`). */
const MARK_CLASS = "wavy-mark";
/** Wait after the session saw the text change (its own check waits 150 ms): about 300 ms after the last key. */
export const PAUSE_MS = 150;
/** Most words the backend checks in one call. */
export const BATCH = 5000;
/** Wait after the caret moved before the word it left is marked (one look per burst of moves). */
export const CARET_MS = 150;

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
  | "rangeAt"
  | "composing"
  | "setNativeSpellcheck"
  | "reveal"
  | "scope"
  | "focus"
>;

/** The word menu the person opened on a marked word (client point of the right-click). */
export type MenuAt = { index: number; word: string; x: number; y: number };

/** The language picker, a menu at a point: the languages, or (in place) the default for new documents. `back`: the
 *  languages again after the default's list (the keyboard starts on the entry that opened it). */
export type PickerAt = { x: number; y: number; level: "languages" | "default"; back: boolean };

/** The download the person is asked to confirm; `declare`: once confirmed, the document is declared in it too. */
export type DownloadAsk = { lang: string; declare: boolean };

/** What the status bar says about the open document's spelling. */
export type SpellingStatus =
  | { kind: "none" }
  | { kind: "unsupported" }
  | {
      kind: "language";
      lang: string;
      source: Exclude<SpellingDocLanguage["source"], "unsupported">;
      /** A language of the document has no dictionary on this computer yet. */
      missing: boolean;
    };

const NO_STATUS: SpellingStatus = { kind: "none" };

export type SpellingDeps = {
  api: Api;
  client: PluginClient;
  session: SpellingSession;
  /** An AI review holds the editor's text (spelling pauses). */
  reviewOpen: () => boolean;
  /** The window's language (the default spelling language follows it until one is picked). */
  uiLocale: () => string;
  /** Says a line to screen readers (the window's polite status line). */
  say: (text: string) => void;
  /** A notice with buttons that takes the place of the one with the same key. */
  notify: (key: string, text: string, ...actions: NoticeAction[]) => void;
  /** Waits between two runs of reading a large text (default: until the page is idle). */
  pause: () => Promise<void>;
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
  /** The corrections of the word the menu was opened on, once the backend answered (the menu shows then). */
  suggested = $state.raw<{ word: string; list: readonly string[] } | null>(null);
  /** The language picker asked for; `null`: closed. */
  picker = $state.raw<PickerAt | null>(null);
  /** The download question shown; `null`: none. */
  asking = $state.raw<DownloadAsk | null>(null);
  /** The app's default language (`null`: not asked yet). */
  defaultLanguage = $state.raw<SpellingDefault | null>(null);
  /** The person's own words (`null`: not asked yet). */
  personal = $state.raw<readonly string[] | null>(null);

  readonly #api: Api;
  readonly #client: PluginClient;
  readonly #session: SpellingSession;
  readonly #reviewOpen: () => boolean;
  readonly #uiLocale: () => string;
  readonly #say: (text: string) => void;
  readonly #notify: SpellingDeps["notify"];
  readonly #pause: SpellingDeps["pause"] | undefined;
  /** The webview's own spellcheck is off (set by this store: only while a dictionary of the document is ready). */
  #nativeOff = false;
  /** The language sample of the text the last pass read (a declare of that same text reuses it). */
  #sample: { text: string; sample: string } | null = null;
  /** The downloads this window started (a failure of one of them is said here, not in every window). */
  readonly #started = new Set<string>();
  /** Every word the last pass found (the marks less the word being typed, while it is held back). */
  #found: readonly Word[] = [];
  /** The word being typed, held back from the marks until the caret leaves it. */
  #held: Word | null = null;
  #caretTimer: ReturnType<typeof setTimeout> | null = null;
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
    this.#say = deps.say ?? ((text) => arrangement.say(text));
    this.#notify =
      deps.notify ?? ((key, text, ...actions) => windowStore.notifyKeyed(key, text, ...actions));
    this.#pause = deps.pause;
  }

  /** The document's languages that have no dictionary on this computer yet. */
  get needsDownload(): string[] {
    const doc = this.document;
    if (doc === null || this.languages.length === 0) return [];
    return doc.language.langs.filter((lang) => !this.#ready(lang));
  }

  /** The open document's spelling for the status bar: nothing while no document is edited. */
  get status(): SpellingStatus {
    const session = this.#session;
    const doc = this.document;
    if (session.path === null || session.mode === "view" || doc?.path !== session.path) {
      return NO_STATUS;
    }
    const { langs, source } = doc.language;
    if (source === "unsupported") return { kind: "unsupported" };
    const [lang] = langs;
    if (lang === undefined) return NO_STATUS;
    return { kind: "language", lang, source, missing: this.needsDownload.length > 0 };
  }

  /** The next misspelling can be stepped to (a document is edited and has marks). */
  get canStep(): boolean {
    return this.#session.mode !== "view" && this.misspelled.length > 0;
  }

  /** `lang` as the backend last told it (`undefined`: not one of its languages). */
  languageOf(lang: string): SpellingLanguage | undefined {
    return this.languages.find((l) => l.id === lang);
  }

  /** Marks the open document until the returned teardown runs (the webview's own spellcheck back on then). */
  start(): () => void {
    const session = this.#session;
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
    listen("spelling:personal", (words) => {
      this.personal = words;
      this.#forget();
    });
    listen("spelling:download", ({ lang, state }) => {
      this.languages = this.languages.map((l) => (l.id === lang ? { ...l, state } : l));
      if (state.kind === "ready") this.#forget();
      if (state.kind !== "downloading" && this.#started.delete(lang) && state.kind === "failed") {
        this.#failed(lang, state.reason);
      }
    });
    const onContextMenu = (event: MouseEvent): void => this.#keyMenu(event);
    const onSelection = (): void => this.#caretMoved();
    window.addEventListener("contextmenu", onContextMenu);
    document.addEventListener("selectionchange", onSelection);
    stops.push(() => {
      window.removeEventListener("contextmenu", onContextMenu);
      document.removeEventListener("selectionchange", onSelection);
    });
    void this.#loadLanguages();
    // bridge from outside stores: the session and the AI review offer no change subscription, so an effect hears
    // their state and hands it to `#follow`; the root is torn down with the plugin (`stopWatch` below)
    const stopWatch = $effect.root(() => {
      // the editor's document, mode and text (after the session's change check) and an AI review
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
      this.#native(true);
      this.#path = null;
      this.#checked = null;
      this.#sample = null;
      this.#asking = null;
      this.#loading = null;
      this.#cache.clear();
      this.#started.clear();
      this.document = null;
      this.languages = [];
      this.picker = null;
      this.asking = null;
      this.defaultLanguage = null;
      this.personal = null;
    };
  }

  /** One pass now over the text on screen (the word at the caret marked too). */
  check(): Promise<void> {
    this.#cancel();
    return this.#pass(false);
  }

  /** Replaces marked word `index` with `text` (one undo step), the caret right after it and the keyboard in the
   *  editor, whether the pick came from the keyboard or the pointer. `false`: nothing changed (composing, the text
   *  moved). */
  replace(index: number, text: string): boolean {
    const word = this.misspelled[index];
    if (word === undefined) return false;
    const session = this.#session;
    const done = session.apply([{ from: word.from, to: word.to, insert: text, expect: word.word }]);
    if (!done) return false;
    this.menuAt = null;
    // Edit remakes the word's whole block, which moves a caret inside it past the block; Source changes only the word
    // and keeps its caret (revealing there would scroll the line to the middle)
    const end = word.from + text.length;
    if (session.mode === "edit") session.reveal({ from: end, to: end });
    // the editor writes its own selection back into the page now: the page selection it left while the menu held the
    // keyboard is stale after the edit, and an editor focused by a plain element focus takes that selection as the
    // person's (a whole paragraph after a keyboard pick)
    session.focus();
    return true;
  }

  /** Accepts `word` in every document from now on (the person's own words, kept in the profile). */
  async addToDictionary(word: string): Promise<boolean> {
    this.menuAt = null;
    const result = await this.#client.call("spelling.personal.add", { word });
    if (!result.ok) return false;
    this.personal = result.value;
    this.#forget();
    return true;
  }

  /** Takes `word` out of the person's own words (marked again where no dictionary knows it). */
  async removeWord(word: string): Promise<boolean> {
    const result = await this.#client.call("spelling.personal.remove", { word });
    if (!result.ok) return false;
    this.personal = result.value;
    this.#forget();
    return true;
  }

  /** Reads the person's own words (settings). */
  async loadPersonal(): Promise<void> {
    const result = await this.#client.call("spelling.personal.list", {});
    if (result.ok) this.personal = result.value;
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
    const text = this.#session.current();
    const sample =
      this.#sample?.text === text
        ? this.#sample.sample
        : sampleOf((await tokenizeInSteps(text, () => false, this.#pause)) ?? []);
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

  /**
   * Selects the next marked word after the caret (wraps around), gives the editor the keyboard and says the word
   * with its first correction. A caret on no mark starts from its selection's end, else from the start of its block
   * (the editors tell no caret offset). `false`: nothing to step to.
   */
  async next(): Promise<boolean> {
    const list = this.misspelled;
    if (!this.canStep) return false;
    const session = this.#session;
    const at = session.markAt(LAYER);
    let index: number;
    if (at !== null) index = (at + 1) % list.length;
    else {
      const from = session.scope("selection")?.to ?? session.scope("paragraph")?.from ?? 0;
      index = Math.max(
        0,
        list.findIndex((word) => word.from >= from),
      );
    }
    const word = list[index];
    if (word === undefined) return false;
    session.reveal({ from: word.from, to: word.to });
    session.focus();
    const [first] = await this.suggestions(word.word);
    this.#say(
      first === undefined
        ? t("spelling.next.saidNone", { word: word.word })
        : t("spelling.next.said", { word: word.word, suggestion: first }),
    );
    return true;
  }

  // ---- the language picker, downloads, the default --------------------------------------------------------------

  /** Opens the language picker at a client point, on one of its lists (default: the languages). */
  openPicker(x: number, y: number, level: PickerAt["level"] = "languages", back = false): void {
    this.picker = { x, y, level, back };
    // another window may have picked a new default meanwhile
    void this.loadDefault();
  }

  closePicker(): void {
    this.picker = null;
  }

  /** Asks the person whether to download `lang`'s dictionary (`declare`: the document takes it once confirmed). */
  askDownload(lang: string, declare: boolean): void {
    this.asking = { lang, declare };
  }

  closeAsk(): void {
    this.asking = null;
  }

  /** The person confirmed the question: the download starts (and the document is declared in it, when asked). */
  async confirmDownload(): Promise<void> {
    const asked = this.asking;
    this.asking = null;
    if (asked === null) return;
    if ((await this.download(asked.lang)) && asked.declare) await this.declare(asked.lang);
  }

  /** Starts the download of `lang`'s dictionary: called only from the person's own confirm | Try again. */
  async download(lang: string): Promise<boolean> {
    this.#started.add(lang);
    const result = await this.#client.call("spelling.download", { lang });
    if (!result.ok) {
      this.#started.delete(lang);
      this.#failed(lang, null);
      return false;
    }
    this.languages = result.value;
    return true;
  }

  /** Stops the download of `lang`'s dictionary (what came so far is let go). */
  async cancelDownload(lang: string): Promise<void> {
    this.#started.delete(lang);
    const result = await this.#client.call("spelling.cancel", { lang });
    if (result.ok) this.languages = result.value;
  }

  /** Reads the app's default language. */
  async loadDefault(): Promise<void> {
    const result = await this.#client.call("spelling.defaults.get", { uiLocale: this.#uiLocale() });
    if (result.ok) this.defaultLanguage = result.value;
  }

  /** Picks the app's default language; a document checked in the default takes the new one. `false`: refused. */
  async setDefault(lang: string): Promise<boolean> {
    const result = await this.#client.call("spelling.defaults.set", { lang });
    if (!result.ok) return false;
    this.defaultLanguage = result.value;
    if (this.document?.language.source === "default") {
      this.document = null;
      this.#asking = null;
      this.#schedule(false, 0);
    }
    return true;
  }

  /** A download this window started failed: said with a way to try again. */
  #failed(lang: string, reason: SpellingFailure | null): void {
    this.#notify(
      `spelling.download.${lang}`,
      t("spelling.failed", {
        language: languageLabel(lang),
        reason: reason === null ? "" : failureText(reason),
      }).trim(),
      { label: t("spelling.retry"), run: () => void this.download(lang) },
    );
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
    if (this.#caretTimer !== null) clearTimeout(this.#caretTimer);
    this.#caretTimer = null;
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
    // a pass waiting to start (the text changed again) makes this one moot
    const stale = (): boolean => seq !== this.#seq || this.#timer !== null;
    const tokens = await tokenizeInSteps(text, stale, this.#pause);
    if (tokens === null || seq !== this.#seq) return;
    const sample = sampleOf(tokens);
    this.#sample = { text, sample };
    const [language] = await Promise.all([this.#languageOf(path, sample), this.#loadLanguages()]);
    if (seq !== this.#seq) return;
    const langs = language?.langs ?? [];
    if (langs.length === 0 || !langs.some((lang) => this.#ready(lang))) {
      this.#native(true);
      this.#clear();
      return;
    }
    this.#native(false);
    const verdicts = await this.#verdicts(langs, tokens);
    if (seq !== this.#seq) return;
    if (verdicts === null) {
      this.#native(true);
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
  async #languageOf(path: string, sample: string): Promise<SpellingDocLanguage | null> {
    const doc = this.document;
    if (doc?.path === path) return doc.language;
    if (this.#asking?.path !== path) {
      const answer = this.#client
        .call("spelling.language", { path, sample, uiLocale: this.#uiLocale() })
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
    // plain loops: a large document has hundreds of thousands of words
    const missing = new Set<string>();
    for (const token of tokens) {
      if (!cache.has(token.word)) missing.add(token.word);
      for (const part of token.parts) if (!cache.has(part.word)) missing.add(part.word);
    }
    const unknown = [...missing];
    for (let at = 0; at < unknown.length; at += BATCH) {
      const words = unknown.slice(at, at + BATCH);
      const result = await this.#client.call("spelling.check", { langs: [...langs], words });
      if (!result.ok) return null;
      words.forEach((word, i) => cache.set(word, result.value[i] ?? true));
    }
    return cache;
  }

  /**
   * The webview's own spellcheck on | off. Off only once a dictionary of the open document's languages is ready, so
   * a person with none (or a writing system none covers) keeps today's check. A pass that cannot tell (another
   * document opening, View, an AI review pausing the marks) leaves it as it is: a review over a document with a ready
   * dictionary keeps it off, so the webview's marks do not flash in and out around the review.
   */
  #native(on: boolean): void {
    if (this.#nativeOff === !on) return;
    this.#nativeOff = !on;
    this.#session.setNativeSpellcheck(on);
  }

  /** Verdicts may have changed (personal words, a dictionary ready): all asked again. */
  #forget(): void {
    this.#cache.clear();
    if (this.#path !== null) this.#schedule(false, 0);
  }

  // ---- marks ---------------------------------------------------------------------------------------------------

  /** Marks `list`; `typing`: the word at the caret is held back (`only`: just when it is still that word). One
   *  drawing: the word at the caret is found before the marks are set. */
  #draw(list: readonly Word[], typing: boolean, only: Word | null = null): void {
    this.#found = list;
    this.#held = null;
    // the word being typed is not done yet
    const at = typing ? this.#session.rangeAt(list) : null;
    const word = at === null ? undefined : list[at];
    if (word === undefined || (only !== null && (word.from !== only.from || word.to !== only.to))) {
      this.#show(list);
      return;
    }
    this.#held = word;
    this.#show(list.filter((_, i) => i !== at));
  }

  /** The caret moved: once it rests, a word held back while typed is marked if the caret left it. */
  #caretMoved(): void {
    if (this.#held === null) return;
    if (this.#caretTimer !== null) clearTimeout(this.#caretTimer);
    this.#caretTimer = setTimeout(() => {
      this.#caretTimer = null;
      this.#release();
    }, CARET_MS);
  }

  /** The held word is marked unless the caret is still in it. The text changed meanwhile: its own pass decides. */
  #release(): void {
    const held = this.#held;
    const session = this.#session;
    if (held === null || session.current() !== this.#checked || session.composing()) return;
    if (session.mode === "view" || this.#reviewOpen()) return;
    this.#draw(this.#found, true, held);
  }

  /** Shift+F10 | the Menu key in an editor: the browser aims its context menu at the editor that has the keyboard,
   *  at its caret (a right-click lands on the mark itself, and the editor opens the menu). Caret on a marked word:
   *  that word's menu, there. */
  #keyMenu(event: MouseEvent): void {
    if (event.defaultPrevented || event.button === 2 || !isEditorTarget(event.target)) return;
    const list = this.misspelled;
    const at = this.#session.markAt(LAYER);
    if (at === null || list[at] === undefined) return;
    event.preventDefault();
    this.#openMenu(list, at, event.clientX, event.clientY);
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
    if (this.suggested?.word === word.word) return;
    this.suggested = null;
    void this.suggestions(word.word).then((found) => {
      if (this.menuAt?.word === word.word) this.suggested = { word: word.word, list: found };
    });
  }

  /** No marks on the layer, no menu. */
  #clear(): void {
    this.menuAt = null;
    this.#found = [];
    this.#held = null;
    this.#show([]);
  }
}

/** The keyboard is in an editor's text (not a field). */
function isEditorTarget(target: EventTarget | null): boolean {
  return (
    isTextTarget(target) &&
    !(target instanceof HTMLInputElement) &&
    !(target instanceof HTMLTextAreaElement)
  );
}

/** The sentence for why a dictionary did not arrive. */
function failureKey(reason: SpellingFailure): MessageKey {
  switch (reason.kind) {
    case "offline":
      return "spelling.failed.offline";
    case "http":
      return "spelling.failed.http";
    case "hash":
      return "spelling.failed.hash";
    case "disk":
      return "spelling.failed.disk";
    case "unreadable":
      return "spelling.failed.unreadable";
    default:
      return reason satisfies never;
  }
}

/** Why a dictionary did not arrive, in words. */
export function failureText(reason: SpellingFailure): string {
  return t(failureKey(reason), { status: reason.kind === "http" ? reason.status : 0 });
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

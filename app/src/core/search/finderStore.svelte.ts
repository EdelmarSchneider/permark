// State of the Search everywhere box (one per open): the words, the scope, the answers the backend gave, the row the
// keyboard is on. The rows come from the pure model; this loads the data they need and acts on a chosen row.

import type { Api, ApiError } from "../../api";
import type { CommandEntry } from "../commands/commands";
import { matchCommands } from "../commands/commands";
import type { MessageKey } from "../i18n";
import type { TagHit } from "./tags.svelte";
import {
  RECENT_LIMIT,
  RESULT_LIMIT,
  applyPrefix,
  buildView,
  firstScope,
  moveActive,
  NO_DATA,
  scopesFor,
  stepScope,
  type Data,
  type Preset,
  type Row,
  type Scope,
  type SectionRow,
} from "./finderModel";

/** Wait after the last key before the backend is asked. */
export const DEBOUNCE_MS = 70;
/** Sections the box lists for a query in All | Sections. */
const SECTION_LIMIT = 30;

export type Load =
  | { status: "idle" }
  | { status: "loading" }
  | { status: "ready"; partial: boolean }
  | { status: "failed"; error: ApiError };

/** The tags of the project, when the tags plugin is on: Search everywhere lists them and narrows the search. */
export type FinderTags = {
  /** The tags that match the words (all of them for an empty box). */
  hits(query: string): readonly TagHit[];
  /** The tags the search is narrowed to now. */
  selected(): readonly string[];
  /** Narrows the search to the files that have this tag too. */
  select(name: string): void;
};

/** What the box needs from its window. */
export type FinderHost = {
  api: Api;
  tags?: FinderTags;
  singleFile: boolean;
  /** The commands of the palette, now. */
  commands(): CommandEntry[];
  label(key: MessageKey): string;
  /** The headings of the open document. */
  outline(): SectionRow[];
  /** Files opened in this project, newest first. */
  opened(): readonly string[];
  openFile(path: string): void;
  reveal(path: string): void;
  openSection(row: SectionRow): void;
  runCommand(entry: CommandEntry): void;
  close(): void;
};

export class FinderStore {
  readonly preset: Preset;
  readonly scopes: readonly Scope[];
  scope = $state<Scope>("commands");
  query = $state("");
  active = $state(0);
  data = $state.raw<Data>(NO_DATA);
  load = $state.raw<Load>({ status: "idle" });
  readonly #host: FinderHost;
  #seq = 0;
  #timer: ReturnType<typeof setTimeout> | null = null;

  constructor(preset: Preset, host: FinderHost) {
    this.preset = preset;
    this.#host = host;
    this.scopes = scopesFor(preset, host.singleFile, host.tags !== undefined);
    this.scope = firstScope(this.scopes);
    this.#refresh(true);
  }

  /** Rows, groups and the empty state for the words and scope now. */
  view = $derived.by(() =>
    buildView({
      preset: this.preset,
      scope: this.scope,
      query: this.query,
      data: this.data,
      commands: matchCommands(this.#host.commands(), this.query, (k) => this.#host.label(k)),
      outline: this.#host.outline(),
      tags: this.scope === "all" || this.scope === "tags" ? this.#host.tags?.hits(this.query) : [],
      singleFile: this.#host.singleFile,
    }),
  );

  /** The row the keyboard is on (the list can be shorter than `active` for a moment). */
  get current(): Row | null {
    return this.view.rows[Math.min(this.active, this.view.rows.length - 1)] ?? null;
  }

  setQuery(text: string): void {
    const next = applyPrefix(this.scopes, this.scope, text);
    this.scope = next.scope;
    this.query = next.text;
    this.active = 0;
    this.#refresh(false);
  }

  setScope(scope: Scope): void {
    if (!this.scopes.includes(scope) || scope === this.scope) return;
    this.scope = scope;
    this.active = 0;
    this.#refresh(true);
  }

  cycle(delta: 1 | -1): void {
    this.setScope(stepScope(this.scopes, this.scope, delta));
  }

  move(delta: 1 | -1): void {
    this.active = moveActive(
      Math.min(this.active, this.view.rows.length - 1),
      delta,
      this.view.rows.length,
    );
  }

  retry(): void {
    this.#refresh(true);
  }

  /** Enter (or a click) on a row; `reveal`: Ctrl+Enter, show the file in the Explorer instead of opening it. */
  activate(row: Row | null = this.current, reveal = false): void {
    if (row === null) return;
    switch (row.type) {
      case "file":
        if (reveal) this.#host.reveal(row.path);
        else this.#host.openFile(row.path);
        this.#host.close();
        break;
      case "folder":
        this.#host.reveal(row.path);
        this.#host.close();
        break;
      case "section":
        this.#host.openSection(row);
        this.#host.close();
        break;
      case "tag":
        // the box stays: the search goes on narrowed to the tag
        this.#host.tags?.select(row.name);
        this.query = "";
        this.active = 0;
        this.setScope(this.scopes.includes("all") ? "all" : this.scope);
        this.#refresh(true);
        break;
      case "command":
        if (row.entry.availability.status === "unavailable") return;
        this.#host.close();
        this.#host.runCommand(row.entry);
        break;
      case "more":
        this.setScope(row.scope);
        break;
      default:
        row satisfies never;
    }
  }

  dispose(): void {
    this.#seq += 1;
    if (this.#timer !== null) clearTimeout(this.#timer);
    this.#timer = null;
  }

  // ---- data ----------------------------------------------------------------------------------------------------

  #refresh(now: boolean): void {
    if (this.#timer !== null) clearTimeout(this.#timer);
    this.#timer = null;
    const seq = ++this.#seq;
    if (
      this.scope === "commands" ||
      this.scope === "tags" ||
      (this.scope === "sections" && this.#host.singleFile)
    ) {
      this.data = NO_DATA;
      this.load = { status: "ready", partial: false };
      return;
    }
    const run = (): void => void this.#fetch(seq);
    if (now || this.query.trim() === "") run();
    else this.#timer = setTimeout(run, DEBOUNCE_MS);
  }

  async #fetch(seq: number): Promise<void> {
    const { api } = this.#host;
    const query = this.query.trim();
    const scope = this.scope;
    this.load = { status: "loading" };
    const tags = this.#host.tags?.selected() ?? [];
    const wantFiles = scope === "all" || scope === "files";
    const wantSections = (scope === "all" || scope === "sections") && query !== "";
    const [paths, recent, sections] = await Promise.all([
      wantFiles && query !== ""
        ? api.searchFiles(
            query,
            scope === "files" ? RESULT_LIMIT : 6,
            this.preset === "quick" ? 0 : 3,
            // a name the person typed is found even if its folder is silenced
            { includeSilenced: true, tags: [...tags] },
          )
        : null,
      wantFiles && query === ""
        ? api.searchRecent([...this.#host.opened()], RECENT_LIMIT * 2, {
            includeSilenced: false,
            tags: [...tags],
          })
        : null,
      wantSections
        ? api.searchSections(query, SECTION_LIMIT, { includeSilenced: false, tags: [...tags] })
        : null,
    ]);
    if (seq !== this.#seq) return;
    const failed = [paths, recent, sections].find((r) => r !== null && !r.ok);
    if (failed && !failed.ok) {
      this.data = NO_DATA;
      this.load = { status: "failed", error: failed.error };
      return;
    }
    const partial = (paths?.ok && paths.value.partial) || (sections?.ok && sections.value.partial);
    this.data = {
      files: paths?.ok ? paths.value.files : [],
      folders: paths?.ok ? paths.value.folders : [],
      sections: sections?.ok ? sections.value.hits : [],
      recent: recent?.ok ? recent.value : null,
    };
    this.load = { status: "ready", partial: partial === true };
  }
}

// Tags and descriptions. The backend keeps them (`meta.toml`) and the catalog of front matter tags; this mirrors what the
// window shows (chips on the page, a line in the row tooltips, the tags view) and sends the person's edits back.

import type {
  ApiError,
  FileMeta,
  IndexEntry,
  TaggedFile,
  TagsIndex,
  TagsOverview,
  TagStatus,
  TagStyleView,
} from "../../api";
import { apiErrorText } from "../../core/errors";
import { t } from "../../core/i18n";
import { PluginClient, pluginClient, workspaceRoot } from "../../core/plugins/client";
import type { TreeBadge } from "../../core/plugins/registry.svelte";
import { projects } from "../../core/projects/instance.svelte";
import { ownerShort, statusWords } from "../../core/projects/vocabulary";
import { windowStore } from "../../core/window.svelte";
import { filterCounts, tipOf } from "./model";

/** The events of a burst (a scan, a rapid edit) become one reload. */
export const RELOAD_MS = 150;

/** The files the tags view lists for the selected tags. */
export type Listing =
  | { status: "idle" }
  | { status: "loading" }
  | { status: "ready"; files: readonly TaggedFile[] }
  | { status: "failed"; error: ApiError };

export class TagsStore {
  index = $state.raw<TagsIndex | null>(null);
  overview = $state.raw<TagsOverview | null>(null);
  /** The tag editor is open on this file. */
  editing = $state.raw<{ path: string } | null>(null);
  // the tags view
  selected = $state.raw<readonly string[]>([]);
  untaggedOnly = $state(false);
  filter = $state("");
  listing = $state.raw<Listing>({ status: "idle" });
  readonly #client: PluginClient;
  readonly #root: () => string | null;
  readonly #notify: (text: string) => void;
  #timer: ReturnType<typeof setTimeout> | null = null;
  #seq = 0;
  #byPath = $derived(new Map((this.index?.entries ?? []).map((e) => [e.path, e])));

  constructor(
    client: PluginClient = pluginClient,
    root: () => string | null = workspaceRoot,
    notify: (text: string) => void = (text) => windowStore.notifyNote(text),
  ) {
    this.#client = client;
    this.#root = root;
    this.#notify = notify;
  }

  /** Follows the project of the window until the returned teardown runs. */
  start(): () => void {
    return $effect.root(() => {
      $effect(() => {
        if (this.#root() === null) {
          this.#reset();
          return;
        }
        let off: (() => void) | null = null;
        let gone = false;
        void this.#client
          .on("tags:changed", () => this.#schedule())
          .then((stop) => {
            if (gone) stop();
            else off = stop;
          });
        void this.load();
        return () => {
          gone = true;
          off?.();
          this.#cancel();
        };
      });
      return () => this.#reset();
    });
  }

  #reset(): void {
    this.#cancel();
    this.index = null;
    this.overview = null;
    this.editing = null;
    this.selected = [];
    this.untaggedOnly = false;
    this.filter = "";
    this.listing = { status: "idle" };
  }

  #cancel(): void {
    if (this.#timer !== null) clearTimeout(this.#timer);
    this.#timer = null;
  }

  #schedule(): void {
    if (this.#timer !== null) return;
    this.#timer = setTimeout(() => {
      this.#timer = null;
      void this.load();
    }, RELOAD_MS);
  }

  /** Reads the index and the overview again (and the files of the selection). */
  async load(): Promise<void> {
    const root = this.#root();
    const [index, overview] = await Promise.all([
      this.#client.call("tags.index", {}),
      this.#client.call("tags.overview", {}),
    ]);
    if (this.#root() !== root) return;
    if (index.ok && (this.index === null || index.value.rev >= this.index.rev)) {
      this.index = index.value;
    }
    if (overview.ok && (this.overview === null || overview.value.rev >= this.overview.rev)) {
      this.overview = overview.value;
    }
    if (this.selected.length > 0 || this.untaggedOnly) void this.#list();
  }

  // ---- what the window shows ----------------------------------------------------------------------------------

  get status(): TagsIndex["status"] | null {
    return this.index?.status ?? null;
  }

  entryOf(path: string): IndexEntry | undefined {
    return this.#byPath.get(path);
  }

  /** The tags of a file (front matter first). */
  tagsOf(path: string): readonly string[] {
    return this.#byPath.get(path)?.tags ?? [];
  }

  descriptionOf(path: string): string {
    return this.#byPath.get(path)?.description ?? "";
  }

  /** The mark of a tag (`projects.md` §7.2) with its words; `null` while the opened folder has no official list. */
  markOf(item: { status: TagStatus | null; owner: string | null }): {
    status: TagStatus;
    words: string;
  } | null {
    if (item.status === null) return null;
    return {
      status: item.status,
      words: statusWords(item.status, "tag", ownerShort(projects.view, item.owner)),
    };
  }

  /** A tag of the opened folder's official list can be made official from the tags view: its list exists (or can be
   *  made: the folder includes projects). A plain folder never offers it. */
  get promotable(): boolean {
    const view = projects.view;
    if (!view?.present) return false;
    return (
      view.vocabulary.active ||
      view.vocabulary.file.status !== "absent" ||
      view.record.status !== "absent"
    );
  }

  styleOf(tag: string): TagStyleView | undefined {
    return this.index?.styles[tag];
  }

  /** A line in the row's tooltip: the tags and the description. Nothing drawn. */
  badge(path: string, isDir: boolean): TreeBadge | null {
    if (isDir) return null;
    const entry = this.#byPath.get(path);
    if (!entry) return null;
    const tip = tipOf(entry.tags, entry.description, t("tags.tooltipLabel"));
    return tip === "" ? null : { text: "", tip };
  }

  /** Every tag with its count, filtered by the box of the tags view. */
  get listed() {
    return filterCounts(this.overview?.tags ?? [], this.filter);
  }

  // ---- the tags view ------------------------------------------------------------------------------------------

  /** A click on a tag selects only it; with Ctrl | Cmd it joins (or leaves) the selection. */
  pick(name: string, multi: boolean): void {
    this.untaggedOnly = false;
    if (multi) {
      this.selected = this.selected.includes(name)
        ? this.selected.filter((n) => n !== name)
        : [...this.selected, name];
    } else {
      this.selected = this.selected.length === 1 && this.selected[0] === name ? [] : [name];
    }
    void this.#list();
  }

  pickUntagged(): void {
    this.selected = [];
    this.untaggedOnly = !this.untaggedOnly;
    void this.#list();
  }

  clearSelection(): void {
    this.selected = [];
    this.untaggedOnly = false;
    this.listing = { status: "idle" };
  }

  async #list(): Promise<void> {
    // an older question still on its way must not answer this one
    const seq = ++this.#seq;
    if (this.selected.length === 0 && !this.untaggedOnly) {
      this.listing = { status: "idle" };
      return;
    }
    if (this.listing.status !== "ready") this.listing = { status: "loading" };
    const result = await this.#client.call("tags.filesWith", {
      tags: [...this.selected],
      untagged: this.untaggedOnly,
    });
    if (seq !== this.#seq) return;
    this.listing = result.ok
      ? { status: "ready", files: result.value }
      : { status: "failed", error: result.error };
  }

  // ---- edits --------------------------------------------------------------------------------------------------

  openEditor(path: string): void {
    this.editing = { path };
  }

  closeEditor(): void {
    this.editing = null;
  }

  async fileMeta(path: string): Promise<FileMeta | null> {
    const result = await this.#client.call("tags.fileMeta", { path });
    return result.ok ? result.value : null;
  }

  /** Replaces the app's tags of a file. The answer is the file's tags as they are now (`null`: refused). */
  async setTags(path: string, tags: readonly string[]): Promise<FileMeta | null> {
    const result = await this.#client.call("tags.setTags", { path, tags: [...tags] });
    if (result.ok) return result.value;
    this.#notify(t("tags.saveFailed", { message: apiErrorText(result.error) }));
    return null;
  }

  async setDescription(path: string, description: string): Promise<FileMeta | null> {
    const result = await this.#client.call("tags.setDescription", { path, description });
    if (result.ok) return result.value;
    this.#notify(t("tags.saveFailed", { message: apiErrorText(result.error) }));
    return null;
  }

  async setStyle(tag: string, color: string | null, icon: string | null): Promise<void> {
    const result = await this.#client.call("tags.setStyle", { tag, color, icon });
    if (!result.ok) this.#notify(t("tags.saveFailed", { message: apiErrorText(result.error) }));
  }

  async orphans(): Promise<readonly string[]> {
    const result = await this.#client.call("tags.orphans", {});
    return result.ok ? result.value : [];
  }

  async cleanUp(): Promise<number> {
    const result = await this.#client.call("tags.cleanUp", {});
    if (result.ok) return result.value;
    this.#notify(t("tags.saveFailed", { message: apiErrorText(result.error) }));
    return 0;
  }
}

export const tags = new TagsStore();

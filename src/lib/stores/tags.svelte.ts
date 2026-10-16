// Tags and descriptions: the per-file index behind filters and tooltips, the Tags view's data, and the
// tags of the open document. The backend does the counting; this store keeps the answers fresh.

import { api } from "../api";
import { errorMessage } from "../errors";
import { t } from "../i18n";
import {
  countTags,
  isEmptySelection,
  NO_SELECTION,
  pruneSelection,
  withAppTags,
  type NamedCount,
  type TagSelection,
} from "../tagModel";
import {
  hasLook,
  NO_STYLES,
  withStyle,
  type TagColor,
  type TagIcon,
  type TagStyle,
  type TagStyles,
} from "../tagStyle";
import type { FileMeta, IndexEntry, TaggedFile, TagOverview } from "../types";
import { toast } from "./toast.svelte";
import { ui } from "./ui.svelte";

/** Changes of files arrive in bursts; the lists are refreshed once they settle. */
const REFRESH_MS = 400;

class TagsStore {
  /** Every file that has a tag or a description, by path. */
  index = $state.raw<ReadonlyMap<string, IndexEntry>>(new Map());
  /** Tags with their number of files, from the index (most files first). */
  counts = $derived<NamedCount[]>(countTags(this.index.values()));
  /** Names of all tags, for "No tag 'x'" checks. */
  known = $derived<ReadonlySet<string>>(new Set(this.counts.map((c) => c.name)));
  /** The Tags view's numbers and the state of `meta.toml`. */
  overview = $state.raw<TagOverview | null>(null);
  /** The color and icon of the tags that have a look (every chip and list of tags reads it). */
  styles = $state.raw<TagStyles>(NO_STYLES);
  /** The open document's tags; check `meta.path` before using it. */
  meta = $state.raw<FileMeta | null>(null);
  selection = $state.raw<TagSelection>(NO_SELECTION);
  /** Files that match the selection. */
  files = $state.raw<TaggedFile[]>([]);
  filter = $state("");

  private tokens = { index: 0, overview: 0, files: 0, meta: 0, styles: 0 };
  private timer: ReturnType<typeof setTimeout> | null = null;

  reset(): void {
    if (this.timer !== null) clearTimeout(this.timer);
    this.timer = null;
    for (const key of Object.keys(this.tokens) as (keyof typeof this.tokens)[]) this.tokens[key]++;
    this.index = new Map();
    this.styles = NO_STYLES;
    this.overview = null;
    this.meta = null;
    this.selection = NO_SELECTION;
    this.files = [];
    this.filter = "";
  }

  // ---- reading --------------------------------------------------------------------------------

  async refreshIndex(): Promise<void> {
    const token = ++this.tokens.index;
    try {
      const list = await api.metaIndex();
      if (token === this.tokens.index) this.index = new Map(list.map((e) => [e.path, e]));
    } catch {
      // keep what is shown; the next change refreshes it
    }
  }

  async refreshStyles(): Promise<void> {
    const token = ++this.tokens.styles;
    try {
      const styles = await api.tagStyles();
      if (token === this.tokens.styles) this.styles = styles;
    } catch {
      // keep what is shown; the next change refreshes it
    }
  }

  /** How a tag looks, or nothing when it has no color and no icon. */
  styleOf(name: string): TagStyle | undefined {
    return this.styles.get(name);
  }

  async refreshOverview(): Promise<void> {
    const token = ++this.tokens.overview;
    try {
      const overview = await api.listTags();
      if (token !== this.tokens.overview) return;
      this.overview = overview;
      const kept = pruneSelection(this.selection, overview.tags);
      if (kept !== this.selection) {
        this.selection = kept;
        void this.refreshFiles();
      }
    } catch {
      // keep what is shown
    }
  }

  async refreshFiles(): Promise<void> {
    const token = ++this.tokens.files;
    const sel = this.selection;
    if (isEmptySelection(sel)) {
      this.files = [];
      return;
    }
    try {
      const files = await api.filesWithTags(sel.tags, sel.untagged);
      if (token === this.tokens.files) this.files = files;
    } catch {
      // keep what is shown
    }
  }

  /** The tags of a document, loaded before the document itself shows (so the page does not jump). */
  async preload(path: string): Promise<void> {
    const token = ++this.tokens.meta;
    try {
      const meta = await api.fileMeta(path);
      if (token === this.tokens.meta) this.meta = meta;
    } catch {
      if (token === this.tokens.meta) this.meta = null;
    }
  }

  async refreshMeta(): Promise<void> {
    const path = this.meta?.path;
    if (path) await this.preload(path);
  }

  /** Something changed that the tag lists may show: refreshes what is on screen. */
  refreshAll(): void {
    void this.refreshIndex();
    void this.refreshStyles();
    void this.refreshMeta();
    if (ui.sidebarView === "tags" || ui.tagEditor !== null) void this.refreshOverview();
    if (ui.sidebarView === "tags") void this.refreshFiles();
  }

  /** `meta-changed`: the tags or descriptions changed (here, or in `meta.toml` on disk). */
  onMetaChanged(): void {
    this.scheduleRefresh(50);
  }

  /** `fs-change`: file tags (front matter) may have changed. */
  onFilesChanged(): void {
    this.scheduleRefresh(REFRESH_MS);
  }

  private scheduleRefresh(ms: number): void {
    if (this.timer !== null) clearTimeout(this.timer);
    this.timer = setTimeout(() => {
      this.timer = null;
      this.refreshAll();
    }, ms);
  }

  // ---- changing -------------------------------------------------------------------------------

  /** Replaces the app's tags of a file. Resolves to false when the change was refused. */
  async setTags(path: string, names: string[]): Promise<boolean> {
    // shown at once, so a second change made before the answer arrives builds on this one
    if (this.meta?.path === path) this.meta = withAppTags(this.meta, names);
    return this.change(path, () => api.setFileTags(path, names));
  }

  async setDescription(path: string, description: string): Promise<boolean> {
    return this.change(path, () => api.setFileDescription(path, description));
  }

  private async change(path: string, call: () => Promise<FileMeta>): Promise<boolean> {
    const token = ++this.tokens.meta;
    try {
      const meta = await call();
      if (token === this.tokens.meta && this.meta?.path === path) this.meta = meta;
      return true;
    } catch (error) {
      toast.message(t("tags.saveFailed", { message: errorMessage(error) }), "warn");
      await this.preload(path);
      return false;
    }
  }

  /** Gives a tag a color and an icon (`null` for none). Resolves to false when the change was refused. */
  async setStyle(tag: string, color: TagColor | null, icon: TagIcon | null): Promise<boolean> {
    // shown at once, so a preview and a second pick made before the answer arrives build on this one
    const token = ++this.tokens.styles;
    const next: TagStyle = { color, icon };
    this.styles = withStyle(this.styles, tag, hasLook(next) ? next : null);
    try {
      const styles = await api.setTagStyle(tag, color, icon);
      if (token === this.tokens.styles) this.styles = styles;
      return true;
    } catch (error) {
      toast.message(t("tags.saveFailed", { message: errorMessage(error) }), "warn");
      await this.refreshStyles();
      return false;
    }
  }

  async cleanUp(): Promise<void> {
    try {
      await api.cleanUpMeta();
    } catch (error) {
      toast.message(t("tags.saveFailed", { message: errorMessage(error) }), "warn");
    }
    await this.refreshOverview();
  }

  // ---- the Tags view --------------------------------------------------------------------------

  select(selection: TagSelection): void {
    this.selection = selection;
    void this.refreshFiles();
  }
}

export const tags = new TagsStore();

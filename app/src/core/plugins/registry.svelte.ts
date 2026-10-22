// Frontend side of the plugin boundary: typed contributions, the registry the core UI renders from, plugin modules
// (`plugins/<id>/index.ts`, loaded lazily) and their activation with teardown symmetry.

import { untrack, type Component } from "svelte";
import type { Manifest } from "../../api/generated/Manifest";
import type { PluginId } from "../../api/generated/PluginId";
import type { Block, Context, DocumentView, ProjectRow } from "../../api";
import { available, type Requirement } from "../conditions/available";
import type { MessageKey } from "../i18n";
import type {
  EmbedProps,
  FrontMatterProps,
  OutlineWatcherProps,
  PageSelection,
  ViewerProps,
  WorkspaceViewProps,
} from "./props";

/** Lazily loaded UI piece (heavy views load when first needed). */
/** A count on a project's detail card and the way to its items (`projectCount`). */
export type ProjectCount = { text: string; open: () => void };

export type LazyComponent<Props extends Record<string, unknown> = Record<string, never>> =
  () => Promise<{
    default: Component<Props>;
  }>;

/** Owner of the contributions the core itself registers (commands, keys): never a plugin, never toggled. */
export const CORE_OWNER: PluginId = "core";

type Owned = { owner: PluginId; requires: Requirement };

/** What a badge says about a path (a git letter, an unread dot, a count on a folder). */
export type BadgeTone =
  "modified" | "added" | "untracked" | "deleted" | "renamed" | "conflict" | "unread" | "muted";
/**
 * What a plugin says about a path. `text` may be empty when the plugin only adds to the row: `dim` (the Explorer draws
 * the row quieter) and `tip` (a line in the row's tooltip).
 */
export type TreeBadge = {
  text: string;
  tone?: BadgeTone;
  title?: string;
  dim?: boolean;
  tip?: string;
};

/** What a plugin puts in a column of the outline panel: a short text and, when useful, a longer one for the tooltip. */
export type OutlineCell = { text: string; title?: string };

/**
 * A short fact a plugin knows about an included project (its branch, what its repository is doing). `chip`: a state
 * (drawn as a chip, as in the status bar; never cut); `warn`: a problem (a warning mark before `text`); `branch`:
 * `text` is a branch name (a branch mark before it, so it reads apart from the project's short name); `commit`: `text`
 * is a commit (a commit mark before it: not on a branch); `spoken`: what
 * the screen-reader name adds after the visible words (none: nothing); `title`: the tooltip (else `text`).
 */
export type ProjectFact = {
  text: string;
  chip?: boolean;
  warn?: boolean;
  branch?: boolean;
  commit?: boolean;
  spoken?: string;
  title?: string;
};

/** The props of a plugin's block in a project's detail card. */
export type ProjectCardProps = { row: ProjectRow };

/** Everything a plugin can add to the core UI. */
export type Contribution = Owned &
  (
    | {
        kind: "command";
        id: string;
        label: MessageKey;
        run: () => void | Promise<void>;
        /** UI-local visibility (a document is open, ...): false = the command is not offered. */
        when?: () => boolean;
        /** `false`: not listed in the palette (the commands that open the box itself). */
        inPalette?: boolean;
      }
    | { kind: "paletteEntry"; id: string; label: MessageKey; command: string }
    | { kind: "shortcut"; keys: string; command: string }
    | {
        kind: "panel";
        id: string;
        side: "left" | "right";
        label: MessageKey;
        /** A small icon: the tab shows it alone while another tab is the open one and the strip is crowded. */
        icon?: Component<{ size?: number }>;
        view: LazyComponent;
      }
    | {
        kind: "statusItem";
        id: string;
        slot: "left" | "center" | "right";
        order: number;
        view: LazyComponent;
      }
    /** A button in the top bar, between the flexible space and Settings (the Notes); lowest `order` first. */
    | { kind: "topBarItem"; id: string; order: number; view: LazyComponent }
    /** A badge on a row of the Explorer and in the document header. Reads reactive state: the row follows it. */
    | {
        kind: "treeBadge";
        id: string;
        badge: (path: string, isDir: boolean) => TreeBadge | null;
      }
    /**
     * A column right of the outline items (the tokens of each section). `header` null = no column. `cell` and `header`
     * read reactive state. `watcher` is mounted (it draws nothing) while the outline shows, with the items: where the
     * plugin works out its figures, with the effects it needs.
     */
    | {
        kind: "outlineColumn";
        id: string;
        header: () => OutlineCell | null;
        cell: (index: number) => OutlineCell | null;
        watcher: LazyComponent<OutlineWatcherProps>;
      }
    /** A strip under the document header (the review bar, a repository notice). Lowest `order` first. */
    | { kind: "docBar"; id: string; order: number; view: LazyComponent }
    /** Changes the blocks the page shows (the review adds the removed ones and its own bars). Reads reactive state. */
    | {
        kind: "docBlocks";
        id: string;
        transform: (blocks: readonly Block[], view: DocumentView) => readonly Block[];
      }
    /**
     * Draws the body of a text | sheet document instead of its viewer while `when` holds (the review of code). Reads
     * reactive state; the first visible contribution whose `when` is true wins.
     */
    | {
        kind: "bodyOverride";
        id: string;
        when: (view: DocumentView) => boolean;
        view: LazyComponent<ViewerProps>;
      }
    /**
     * Moves to the next (`1`) | previous (`-1`) change of the open document for F7 | Shift+F7. The steps run lowest
     * `order` first; the first that returns true has moved and the rest stay quiet.
     */
    | {
        kind: "changeStep";
        id: string;
        order: number;
        step: (direction: 1 | -1) => boolean | Promise<boolean>;
      }
    /**
     * What a plugin knows about the included projects (git: branch, changes): the muted text of a project node, the
     * status of its Settings row, a line in its expanded row, one sentence for the Settings intro, a block in its detail
     * card. Every function reads reactive state; `null` = nothing to say. The plugin's own `treeBadge` on the project
     * folder is left out of the card and the Settings row (its block | status says it).
     */
    | {
        kind: "projectFacts";
        id: string;
        node: (row: ProjectRow) => ProjectFact | null;
        row: (row: ProjectRow) => ProjectFact | null;
        info: (row: ProjectRow) => { label: string; text: string } | null;
        note: () => string | null;
        card: LazyComponent<ProjectCardProps>;
      }
    /**
     * A short count a plugin keeps about an included project, for the counts line of its detail card ("2 open
     * comments"; joined with " · " after the folder's badge titles, which it replaces for its plugin). `open` shows
     * those items (the plugin's tab, only that project's); the card closes first. Reads reactive state; `null` =
     * nothing to say.
     */
    | { kind: "projectCount"; id: string; count: (row: ProjectRow) => ProjectCount | null }
    /** An entry of the Explorer's context menu (a single row). */
    | {
        kind: "treeMenu";
        id: string;
        label: MessageKey;
        applies: (path: string, isDir: boolean) => boolean;
        run: (paths: readonly string[]) => void;
      }
    /** An entry of the page's context menu while text is selected (comment on the passage). */
    | {
        kind: "selectionMenu";
        id: string;
        label: MessageKey;
        run: (selection: PageSelection) => void;
      }
    /** The controls a plugin puts with the search boxes (tag chips): shown in the Search panel and the finder. */
    | { kind: "searchFilter"; id: string; view: LazyComponent }
    /** Runs on the root of the page when it exists; returns what undoes it. */
    | { kind: "docDecoration"; id: string; decorate: (root: HTMLElement) => () => void }
    | { kind: "settingsSection"; id: string; label: MessageKey; view: LazyComponent }
    /** Rows a plugin adds to the Project section of the settings (what the project shares in `.permark/`). */
    | { kind: "projectSection"; id: string; label: MessageKey; view: LazyComponent }
    /** What the window shows for a workspace of a kind the core has no page for (the Notes): its sidebar and its page. */
    | {
        kind: "workspaceView";
        workspace: "notes";
        sidebar: LazyComponent;
        main: LazyComponent<WorkspaceViewProps>;
      }
    /** A block of the welcome screen (the latest notes). */
    | { kind: "welcomeSection"; id: string; order: number; view: LazyComponent }
    | {
        kind: "viewer";
        id: string;
        claims: (name: string) => boolean;
        view: LazyComponent<ViewerProps>;
      }
    | { kind: "editor"; id: string; claims: (name: string) => boolean; view: LazyComponent }
    | { kind: "embed"; id: string; language: string; view: LazyComponent<EmbedProps> }
    /** A button next to Copy on a fenced code block whose language it takes (convert the block). */
    | {
        kind: "codeAction";
        id: string;
        label: MessageKey;
        icon: Component<{ size?: number }>;
        applies: (language: string) => boolean;
        run: (source: string, language: string) => void;
      }
    | { kind: "frontMatterCard"; id: string; view: LazyComponent<FrontMatterProps> }
  );

export type ContributionKind = Contribution["kind"];
/** A contribution as a plugin hands it over (owner added by the host). */
export type ContributionInput = Contribution extends infer C
  ? C extends Contribution
    ? Omit<C, "owner">
    : never
  : never;
export type ContributionOf<K extends ContributionKind> = Extract<Contribution, { kind: K }>;

export class ContributionRegistry {
  items = $state.raw<readonly Contribution[]>([]);

  /**
   * Adds one contribution; returns its removal. Changing the registry never makes the caller depend on it (an effect
   * that registers would otherwise run again for every registration, and for its own).
   */
  register(contribution: Contribution): () => void {
    untrack(() => {
      this.items = [...this.items, contribution];
    });
    return () => {
      untrack(() => {
        this.items = this.items.filter((item) => item !== contribution);
      });
    };
  }

  /** Removes everything an owner contributed (safety net after deactivate). Returns how many were left. */
  removeOwner(owner: PluginId): number {
    return untrack(() => {
      const left = this.items.filter((item) => item.owner === owner).length;
      if (left > 0) this.items = this.items.filter((item) => item.owner !== owner);
      return left;
    });
  }

  ownedBy(owner: PluginId): readonly Contribution[] {
    return this.items.filter((item) => item.owner === owner);
  }

  of<K extends ContributionKind>(kind: K): ContributionOf<K>[] {
    return this.items.filter((item): item is ContributionOf<K> => item.kind === kind);
  }

  /** Contributions the UI may render in `context` (palette lists unavailable ones too, with their reason). */
  visible<K extends ContributionKind>(kind: K, context: Context): ContributionOf<K>[] {
    return this.of(kind).filter(
      (item) => available(item.requires, context).status !== "unavailable",
    );
  }
}

/** What an active plugin may use. */
export type PluginContext = {
  id: PluginId;
  /** Registers a contribution owned by this plugin; returns its removal. */
  contribute(contribution: ContributionInput): () => void;
};

/** Teardown returned by `activate`: MUST remove every contribution + listener it added. */
export type Deactivate = () => void;

export type PluginModule = {
  manifest: Pick<Manifest, "id" | "kind" | "dependsOn">;
  activate(cx: PluginContext): Deactivate;
};

export function definePlugin(module: PluginModule): PluginModule {
  return module;
}

type Loader = () => Promise<{ default: PluginModule }>;

/** Plugin modules by id, from `plugins/<id>/index.ts` (each a separate chunk). */
export function pluginLoaders(): Record<string, Loader> {
  const found = import.meta.glob<{ default: PluginModule }>("../../plugins/*/index.ts");
  const loaders: Record<string, Loader> = {};
  for (const [path, loader] of Object.entries(found)) {
    const id = path.split("/").at(-2);
    if (id) loaders[id] = loader;
  }
  return loaders;
}

type PluginState =
  | { status: "off" }
  | { status: "loading"; seq: number }
  | { status: "on"; deactivate: Deactivate }
  | { status: "failed"; error: string };

/** Activates | deactivates plugin modules. Rapid toggles: only the latest wish wins (stale loads never activate). */
export class PluginHost {
  #states = new Map<PluginId, PluginState>();
  #seq = 0;
  readonly registry: ContributionRegistry;
  readonly #loaders: Record<string, Loader>;

  constructor(registry: ContributionRegistry, loaders: Record<string, Loader>) {
    this.registry = registry;
    this.#loaders = loaders;
  }

  /** Whether the UI has a module for this plugin (backend-only plugins have none). */
  has(id: PluginId): boolean {
    return Object.hasOwn(this.#loaders, id);
  }

  state(id: PluginId): PluginState["status"] {
    return this.#states.get(id)?.status ?? "off";
  }

  async enable(id: PluginId): Promise<void> {
    const current = this.#states.get(id);
    if (current?.status === "on" || current?.status === "loading") return;
    const loader = this.#loaders[id];
    if (!loader) {
      this.#states.set(id, { status: "failed", error: "no module" });
      return;
    }
    const seq = ++this.#seq;
    this.#states.set(id, { status: "loading", seq });
    let module: PluginModule;
    try {
      module = (await loader()).default;
    } catch (error) {
      if (this.#isLatest(id, seq)) this.#states.set(id, { status: "failed", error: String(error) });
      return;
    }
    // disabled (or re-requested) while loading: this load is stale
    if (!this.#isLatest(id, seq)) return;
    const cx: PluginContext = {
      id,
      contribute: (contribution) => this.registry.register({ ...contribution, owner: id }),
    };
    try {
      this.#states.set(id, { status: "on", deactivate: module.activate(cx) });
    } catch (error) {
      this.registry.removeOwner(id);
      this.#states.set(id, { status: "failed", error: String(error) });
    }
  }

  /** Deactivates; returns how many contributions the plugin failed to remove (0 = clean teardown). */
  disable(id: PluginId): number {
    const current = this.#states.get(id);
    this.#states.set(id, { status: "off" });
    if (current?.status === "on") {
      try {
        current.deactivate();
      } catch {
        // a throwing teardown still loses its contributions below
      }
    }
    return this.registry.removeOwner(id);
  }

  #isLatest(id: PluginId, seq: number): boolean {
    const state = this.#states.get(id);
    return state?.status === "loading" && state.seq === seq;
  }
}

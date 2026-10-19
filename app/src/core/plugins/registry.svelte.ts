// Frontend side of the plugin boundary: typed contributions, the registry the core UI renders from, plugin modules
// (`plugins/<id>/index.ts`, loaded lazily) and their activation with teardown symmetry.

import { untrack, type Component } from "svelte";
import type { Manifest } from "../../api/generated/Manifest";
import type { PluginId } from "../../api/generated/PluginId";
import type { Block, Context, DocumentView } from "../../api";
import { available, type Requirement } from "../conditions/available";
import type { MessageKey } from "../i18n";
import type { EmbedProps, FrontMatterProps, ViewerProps } from "./props";

/** Lazily loaded UI piece (heavy views load when first needed). */
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
export type TreeBadge = { text: string; tone?: BadgeTone; title?: string };

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
    | { kind: "panel"; id: string; side: "left" | "right"; label: MessageKey; view: LazyComponent }
    | {
        kind: "statusItem";
        id: string;
        slot: "left" | "center" | "right";
        order: number;
        view: LazyComponent;
      }
    /** A badge on a row of the Explorer and in the document header. Reads reactive state: the row follows it. */
    | {
        kind: "treeBadge";
        id: string;
        badge: (path: string, isDir: boolean) => TreeBadge | null;
      }
    /** A strip under the document header (the review bar, a repository notice). Lowest `order` first. */
    | { kind: "docBar"; id: string; order: number; view: LazyComponent }
    /** Changes the blocks the page shows (the review adds the removed ones and its own bars). Reads reactive state. */
    | {
        kind: "docBlocks";
        id: string;
        transform: (blocks: readonly Block[], view: DocumentView) => readonly Block[];
      }
    /** An entry of the Explorer's context menu (a single row). */
    | {
        kind: "treeMenu";
        id: string;
        label: MessageKey;
        applies: (path: string, isDir: boolean) => boolean;
        run: (paths: readonly string[]) => void;
      }
    /** Runs on the root of the page when it exists; returns what undoes it. */
    | { kind: "docDecoration"; id: string; decorate: (root: HTMLElement) => () => void }
    | { kind: "settingsSection"; id: string; label: MessageKey; view: LazyComponent }
    | {
        kind: "viewer";
        id: string;
        claims: (name: string) => boolean;
        view: LazyComponent<ViewerProps>;
      }
    | { kind: "editor"; id: string; claims: (name: string) => boolean; view: LazyComponent }
    | { kind: "embed"; id: string; language: string; view: LazyComponent<EmbedProps> }
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

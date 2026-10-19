// Review: what changed in the open document since the person last read it, shown in the page with Keep | Revert for
// each change. One small machine: `idle` (nothing to review) | `offer` (changes exist: the bar offers them) |
// `reviewing` (the page shows them). The backend owns the truth (copies, the diff, the surgical save).

import { untrack } from "svelte";
import type { Basis, Result, ReviewView } from "../../api";
import { documents } from "../../core/doc/document.svelte";
import { t, tn } from "../../core/i18n";
import { PluginClient, pluginClient, workspaceRoot } from "../../core/plugins/client";
import { windowStore } from "../../core/window.svelte";
import { kindWord } from "./blocks";

export type ReviewUi =
  | { status: "idle" }
  | { status: "offer"; path: string; count: number }
  | { status: "reviewing"; view: ReviewView; at: number };

const MARKDOWN = /\.(md|markdown|mdown|mkd|mdx|mdc)$/i;

/** The kind of change `i` in words (for screen readers). */
function kindAt(view: ReviewView, i: number): string {
  const change = view.changes[i];
  return change ? kindWord(change.kind) : "";
}

/** What the store needs of the page: the document shown (reactive), and to make the blocks again. */
export type PagePort = {
  shown(): { path: string | null; hash: string | null; markdown: boolean };
  repaint(): void;
};

const page: PagePort = {
  shown: () => {
    const path = documents.path;
    const view = documents.view;
    const mine = view !== null && view.path === path;
    return {
      path,
      hash: mine ? view.hash : null,
      markdown: mine && view.format === "markdown" && path !== null && MARKDOWN.test(path),
    };
  },
  repaint: () => documents.refreshBlocks(),
};

export class ReviewStore {
  state = $state.raw<ReviewUi>({ status: "idle" });
  /** What documents are compared with. */
  basis = $state<Basis>("read");
  /** Keeps and reverts of this review that Undo can take back. */
  undoable = $state(0);
  /** The latest thing said for screen readers (a live region in the bar). */
  announcement = $state("");
  #seq = 0;
  /** The document an `offer` | `reviewing` belongs to (to release it when the person leaves). */
  #held: string | null = null;
  readonly #client: PluginClient;
  readonly #root: () => string | null;
  readonly #say: (text: string) => void;
  readonly #page: PagePort;

  constructor(
    client: PluginClient = pluginClient,
    root: () => string | null = workspaceRoot,
    say: (text: string) => void = (text) => windowStore.notifyNote(text),
    port: PagePort = page,
  ) {
    this.#client = client;
    this.#root = root;
    this.#say = say;
    this.#page = port;
  }

  get reviewing(): ReviewView | null {
    return this.state.status === "reviewing" ? this.state.view : null;
  }

  /** The change the keys act on (index), or -1. */
  get current(): number {
    return this.state.status === "reviewing" ? this.state.at : -1;
  }

  /** Follows the document of the window until the returned teardown runs. */
  start(): () => void {
    return $effect.root(() => {
      $effect(() => {
        if (this.#root() === null) return;
        void this.#client.call("review.basis", {}).then((r) => {
          if (r.ok) this.basis = r.value;
        });
      });
      // the document shown (or its version) changed
      $effect(() => {
        const { path, hash, markdown } = this.#page.shown();
        untrack(() => void this.#sync(markdown ? path : null, hash));
      });
      return () => this.#reset();
    });
  }

  #reset(): void {
    this.#seq += 1;
    this.#held = null;
    this.state = { status: "idle" };
    this.undoable = 0;
  }

  async #sync(path: string | null, hash: string | null): Promise<void> {
    const seq = ++this.#seq;
    const reviewing = this.reviewing;
    if (this.#held !== null && this.#held !== path) {
      // the person left the document: its review is over
      void this.#client.call("review.release", { path: this.#held });
      this.#held = null;
      this.undoable = 0;
    }
    if (path === null || hash === null) {
      this.state = { status: "idle" };
      this.#page.repaint();
      return;
    }
    if (reviewing !== null && reviewing.path === path) {
      if (reviewing.hash === hash) return;
      // the file changed under an open review: look again, keep the place
      const at = this.current;
      const result = await this.#client.call("review.open", { path });
      if (seq !== this.#seq) return;
      if (result.ok && result.value.count > 0) {
        this.state = {
          status: "reviewing",
          view: result.value,
          at: Math.min(at, result.value.count - 1),
        };
        this.announce(tn("review.sr.updated", result.value.count));
      } else {
        this.state = { status: "idle" };
        this.#held = null;
      }
      this.#page.repaint();
      return;
    }
    const status = await this.#client.call("review.status", { path });
    if (seq !== this.#seq) return;
    if (status.ok && status.value.state.state === "changes") {
      this.#held = path;
      this.state = { status: "offer", path, count: status.value.state.count };
    } else {
      this.state = { status: "idle" };
    }
    if (status.ok) this.basis = status.value.basis;
    this.#page.repaint();
  }

  announce(text: string): void {
    this.announcement = text;
  }

  /** Shows the changes in the page. */
  async begin(): Promise<void> {
    const { path, markdown } = this.#page.shown();
    if (path === null || !markdown) return;
    const status = await this.#client.call("review.status", { path });
    if (!status.ok) return;
    const state = status.value.state;
    if (state.state === "tooLarge") return this.#say(t("review.tooLarge"));
    if (state.state === "noBasis") return this.#say(t("review.noBaseline"));
    if (state.state === "same") return this.#say(t("review.allDone"));
    if (state.state === "unsupported") return;
    const result = await this.#client.call("review.open", { path });
    if (!result.ok) return;
    this.#held = path;
    this.undoable = 0;
    this.#show(result.value, 0);
    this.announce(tn("review.sr.opened", result.value.count, { kind: kindAt(result.value, 0) }));
  }

  #show(view: ReviewView, at: number): void {
    this.state = {
      status: "reviewing",
      view,
      at: view.count === 0 ? -1 : Math.max(0, Math.min(at, view.count - 1)),
    };
    this.#page.repaint();
  }

  /** Closes the review: what is shown is what was read. */
  async done(): Promise<void> {
    const path = this.reviewing?.path ?? this.#held;
    this.#seq += 1;
    this.state = { status: "idle" };
    this.undoable = 0;
    this.#held = null;
    this.#page.repaint();
    if (path !== null) await this.#client.call("review.release", { path });
  }

  async keep(index = this.current): Promise<void> {
    const view = this.reviewing;
    if (view === null || index < 0 || !view.canKeep) return;
    const result = await this.#client.call("review.keep", {
      path: view.path,
      hash: view.hash,
      index,
      basis: this.basis,
    });
    this.#after(result, index, "kept", true);
  }

  async revert(index = this.current): Promise<void> {
    const view = this.reviewing;
    if (view === null || index < 0) return;
    const result = await this.#client.call("review.revert", {
      path: view.path,
      hash: view.hash,
      index,
      basis: this.basis,
    });
    this.#after(result, index, "reverted", true);
  }

  async keepAll(): Promise<void> {
    const view = this.reviewing;
    if (view === null || !view.canKeep) return;
    const result = await this.#client.call("review.keepAll", {
      path: view.path,
      hash: view.hash,
      basis: this.basis,
    });
    this.#after(result, 0, "keptAll", false);
  }

  async revertAll(): Promise<void> {
    const view = this.reviewing;
    if (view === null) return;
    const result = await this.#client.call("review.revertAll", {
      path: view.path,
      hash: view.hash,
      basis: this.basis,
    });
    this.#after(result, 0, "revertedAll", false);
  }

  async undo(): Promise<void> {
    const view = this.reviewing;
    if (view === null || this.undoable === 0) return;
    const result = await this.#client.call("review.undo", { path: view.path, basis: this.basis });
    if (!result.ok) {
      this.#say(t("review.undoStale", { name: view.path.split("/").at(-1) ?? view.path }));
      return;
    }
    this.undoable -= 1;
    this.#show(result.value, this.current);
    this.announce(tn("review.sr.undone", result.value.count));
  }

  /** What a keep | revert answered: the review goes on, or ends when nothing is left. */
  #after(
    result: Result<ReviewView>,
    index: number,
    what: "kept" | "reverted" | "keptAll" | "revertedAll",
    single: boolean,
  ): void {
    const view = this.reviewing;
    const name = view?.path.split("/").at(-1) ?? "";
    if (!result.ok) {
      const error = result.error;
      if (error.kind === "backend" && error.error.kind === "conflict") {
        // the file is not the version the changes were drawn from: look again
        this.#say(t("review.stale", { name }));
        if (view !== null) void this.#refresh(view.path);
      } else {
        this.#say(
          t("review.failed", {
            name,
            message: error.kind === "backend" ? error.error.kind : error.message,
          }),
        );
      }
      return;
    }
    if (single) this.undoable += 1;
    else this.undoable = 1;
    const next = result.value;
    if (next.count === 0) {
      this.announce(t("review.allDone"));
      this.#say(
        t(what === "keptAll" || what === "kept" ? "review.keptAll" : "review.revertedAll", {
          name,
        }),
      );
      this.state = { status: "idle" };
      this.#held = null;
      this.undoable = 0;
      this.#page.repaint();
      return;
    }
    this.#show(next, single ? index : 0);
    this.announce(
      what === "kept"
        ? tn("review.sr.kept", next.count)
        : what === "reverted"
          ? tn("review.sr.reverted", next.count)
          : tn("review.sr.opened", next.count, { kind: kindAt(next, 0) }),
    );
  }

  async #refresh(path: string): Promise<void> {
    const result = await this.#client.call("review.open", { path });
    if (result.ok && result.value.count > 0) this.#show(result.value, this.current);
    else {
      this.state = { status: "idle" };
      this.#page.repaint();
    }
  }

  go(step: 1 | -1): void {
    const view = this.reviewing;
    if (view === null || view.count === 0) return;
    const at = (this.current + step + view.count) % view.count;
    this.state = { status: "reviewing", view, at };
    this.announce(
      t("review.sr.at", {
        i: at + 1,
        n: view.count,
        kind: kindAt(view, at),
      }),
    );
    this.#page.repaint();
  }

  async setBasis(basis: Basis): Promise<void> {
    if (basis === this.basis) return;
    this.basis = basis;
    await this.#client.call("review.setBasis", { basis });
    const view = this.reviewing;
    if (view === null) return;
    const result = await this.#client.call("review.open", { path: view.path, basis });
    if (result.ok) this.#show(result.value, 0);
  }

  /** Writes a deleted document back as it was last read. */
  async restore(path: string): Promise<void> {
    const result = await this.#client.call("review.restore", { path });
    if (result.ok) this.#say(t("review.restored", { name: path.split("/").at(-1) ?? path }));
  }
}

export const review = new ReviewStore();

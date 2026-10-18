// The Broken links panel: every link in the folder whose file is not there, and a repair when a file
// with that name exists somewhere else.

import { api } from "../api";
import { errorMessage } from "../errors";
import { t } from "../i18n";
import { basename } from "../paths";
import type { BrokenReport, BufferText } from "../types";
import { docs } from "./docs.svelte";
import { edit } from "./edit.svelte";
import { session } from "./session.svelte";
import { toast } from "./toast.svelte";
import { ui } from "./ui.svelte";

/** File changes arrive in bursts; the list is refreshed once they settle. */
const REFRESH_MS = 400;

class BrokenLinksStore {
  report = $state.raw<BrokenReport | null>(null);
  status = $state<"idle" | "loading" | "ready" | "error">("idle");
  error = $state<string | null>(null);
  /** `path:start` of the link a repair is writing. */
  repairing = $state<string | null>(null);
  private generation = 0;
  private timer: ReturnType<typeof setTimeout> | null = null;

  /** The panel is showing, so a change on disk should be looked at again. */
  filesChanged(): void {
    if (ui.sidebarView !== "links" || !session.left.open) return;
    if (this.timer !== null) clearTimeout(this.timer);
    this.timer = setTimeout(() => {
      this.timer = null;
      void this.refresh();
    }, REFRESH_MS);
  }

  async refresh(): Promise<void> {
    const gen = ++this.generation;
    this.status = "loading";
    this.error = null;
    const buffer = edit.unsavedBuffer();
    try {
      const report = await api.brokenLinks(buffer ? [buffer] : []);
      if (gen !== this.generation) return;
      this.report = report;
      this.status = "ready";
    } catch (error) {
      if (gen !== this.generation) return;
      this.report = null;
      this.status = "error";
      this.error = errorMessage(error);
    }
  }

  /** Points `link` at `target`. A document with unsaved Visual edits is left alone. */
  async repair(path: string, start: number, end: number, target: string): Promise<void> {
    const key = `${path}:${start}`;
    if (this.repairing !== null) return;
    const blocked = edit.unsavedUnpatchable();
    if (blocked === path) {
      toast.message(t("links.unsaved", { name: basename(path) }), "warn");
      return;
    }
    const buffer = editorBuffer(path);
    this.repairing = key;
    try {
      const outcome = await api.repairLink(path, start, end, target, buffer);
      if (outcome.status === "stale") {
        toast.message(t("links.stale"), "warn");
      } else if (outcome.status === "notAMatch") {
        toast.message(t("links.failed"), "warn");
      } else if (outcome.buffer && buffer) {
        const applied = edit.applyLinkEdits(outcome.buffer.edits, buffer.text);
        if (!applied) toast.message(t("links.stale"), "warn");
        else toast.message(t("links.repaired", { path: target }));
      } else {
        await docs.followMoves([]);
        edit.followMoves([]);
        const opId = outcome.opId;
        toast.message(
          t("links.repaired", { path: target }),
          "info",
          opId === null ? undefined : { label: t("fileOps.undo"), run: () => void this.undo(opId) },
        );
      }
    } catch (error) {
      toast.message(t("links.failed"), "warn");
      this.error = errorMessage(error);
    } finally {
      this.repairing = null;
      await this.refresh();
    }
  }

  private async undo(opId: number): Promise<void> {
    try {
      await api.undoFileOp(opId);
      await docs.followMoves([]);
      edit.followMoves([]);
    } catch (error) {
      toast.message(errorMessage(error), "warn");
    }
    await this.refresh();
  }
}

/** The open document's unsaved Source text, when the repair is about that document. */
function editorBuffer(path: string): BufferText | null {
  const buffer = edit.unsavedBuffer();
  return buffer !== null && buffer.path === path ? buffer : null;
}

export const brokenLinks = new BrokenLinksStore();

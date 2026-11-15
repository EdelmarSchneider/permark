// The window's half of a PDF | HTML | print: the page setup dialog (PDF and print share it), then one run at a time:
// build the page, hand it to the backend (Save dialog | print dialog), tell how it went. The saved file is the source:
// a document with unsaved changes is refused, never exported from its old text.

import { api, type ApiError } from "../../api";
import type { PageBox } from "../../api/generated/PageBox";
import type { PageSetup } from "../../api/generated/PageSetup";
import type { PrintCapabilities } from "../../api/generated/PrintCapabilities";
import { documents } from "../../core/doc/document.svelte";
import type { EnhanceEnv } from "../../core/doc/enhance";
import { editing } from "../../core/editing/session.svelte";
import { apiErrorText } from "../../core/errors";
import { t, tn, type MessageKey } from "../../core/i18n";
import { pluginClient, type PluginClient } from "../../core/plugins/client";
import { windowStore, type NoticeAction, type NoticeGone } from "../../core/window.svelte";
import type { ExportOptions, ExportOutcome, ExportWords } from "./builder";
import { offeredSetup } from "./setup";

/** What the person asked for: the entry of the document's menu (or the shortcut) they came from. */
export type Entry = "pdf" | "html" | "print";
/** The entries that go through the page setup dialog. */
export type PaperEntry = Exclude<Entry, "html">;

export type ExportState =
  | { status: "closed" }
  | {
      status: "setup";
      entry: PaperEntry;
      path: string;
      setup: PageSetup;
      capabilities: PrintCapabilities;
      /** The setup is being remembered (the dialog's buttons wait). */
      busy: boolean;
    }
  | { status: "running"; entry: Entry; path: string };

/** The notices the flow shows (the window's notice area). */
export type ExportNotices = {
  notifyKeyed(key: string, text: string, ...actions: NoticeAction[]): void;
  notifyLasting(body: {
    text: string;
    actions: readonly NoticeAction[];
    onGone: (how: NoticeGone) => void;
  }): number;
  dismiss(id: number): void;
  dismissKey(key: string): void;
};

export type ExportDeps = {
  client: PluginClient;
  build: (path: string, options: ExportOptions) => Promise<ExportOutcome>;
  notices: ExportNotices;
  /** The document has changes not yet saved. */
  unsaved: (path: string) => boolean;
  env: () => EnhanceEnv;
  /** The person's language tag with its region (the paper offered before they chose one). */
  region: () => string;
  /** A quick run shows no progress notice. */
  progressAfterMs: number;
};

const KEY = "export";

/** The builder is loaded on first use: the plugin stays dormant until the person asks. */
async function buildWithPage(path: string, options: ExportOptions): Promise<ExportOutcome> {
  const { buildExport } = await import("./builder");
  return buildExport(api, path, options);
}

function words(): ExportWords {
  return {
    videoLeftOut: (name) => t("export.videoLeftOut", { name }),
    audioLeftOut: (name) => t("export.audioLeftOut", { name }),
    diagramFailed: t("export.diagramFailed"),
  };
}

function fileName(path: string): string {
  return path.split(/[\\/]/).at(-1) ?? path;
}

/** The name the Save dialog offers: the document's, with the output's extension. */
function outputName(path: string, entry: Entry): string {
  return `${fileName(path).replace(/\.[^.]+$/, "")}.${entry === "html" ? "html" : "pdf"}`;
}

function progressKey(entry: Entry): MessageKey {
  switch (entry) {
    case "pdf":
      return "export.progress.pdf";
    case "html":
      return "export.progress.html";
    case "print":
      return "export.progress.print";
    default:
      return entry satisfies never;
  }
}

/** Why a save | print failed, in words that say what to do. */
function failureText(entry: Entry, path: string, error: ApiError): string {
  if (error.kind !== "backend") return apiErrorText(error);
  const { kind, detail } = error.error;
  if (kind === "conflict" && detail === "source") return t("export.isSource");
  if (kind === "unsupported") return t("export.unsupported");
  if (entry === "print") return t("export.printFailed");
  if (entry === "pdf" && kind === "internal") return t("export.pdfFailed");
  return t("export.saveFailed", { name: outputName(path, entry) });
}

export class ExportStore {
  state = $state.raw<ExportState>({ status: "closed" });
  readonly #deps: ExportDeps;
  #run: { controller: AbortController; timer: ReturnType<typeof setTimeout> | null } | null = null;
  #progress: number | null = null;

  constructor(deps: ExportDeps) {
    this.#deps = deps;
  }

  /** The person asked for a PDF | an HTML page | a printout of `path` (the document the window shows). */
  async start(entry: Entry, path: string): Promise<void> {
    if (this.state.status !== "closed") return;
    if (this.#deps.unsaved(path)) {
      this.#deps.notices.notifyKeyed(KEY, t("export.unsaved"));
      return;
    }
    if (entry === "html") {
      await this.#execute("html", path, null);
      return;
    }
    const [view, capabilities] = await Promise.all([
      this.#deps.client.call("export.setup.get", {}),
      this.#deps.client.call("export.capabilities", {}),
    ]);
    if (!view.ok || !capabilities.ok) {
      const error = view.ok ? (capabilities.ok ? null : capabilities.error) : view.error;
      if (error) this.#deps.notices.notifyKeyed(KEY, apiErrorText(error));
      return;
    }
    if (this.state.status !== "closed") return;
    this.state = {
      status: "setup",
      entry,
      path,
      setup: offeredSetup(view.value, this.#deps.region()),
      capabilities: capabilities.value,
      busy: false,
    };
  }

  /** A choice of the dialog changed. */
  change(setup: Partial<PageSetup>): void {
    const s = this.state;
    if (s.status !== "setup") return;
    this.state = { ...s, setup: { ...s.setup, ...setup } };
  }

  /** Save PDF… | Print… of the dialog: the setup is remembered, then the run starts. */
  async go(entry: PaperEntry): Promise<void> {
    const s = this.state;
    if (s.status !== "setup" || s.busy) return;
    this.state = { ...s, busy: true };
    const stored = await this.#deps.client.call("export.setup.set", { setup: s.setup });
    if (this.state.status !== "setup") return;
    if (!stored.ok) {
      this.state = { ...s, busy: false };
      this.#deps.notices.notifyKeyed(KEY, apiErrorText(stored.error));
      return;
    }
    await this.#execute(entry, s.path, {
      setup: s.setup,
      page: stored.value.page,
      capabilities: s.capabilities,
    });
  }

  /**
   * Cancel: the dialog closes; a run stops (the build at its next step; the backend drops a PDF still rendering and
   * closes its Save | print dialog, a file already written stays and is reported).
   */
  cancel(): void {
    const s = this.state;
    if (s.status === "setup") {
      this.state = { status: "closed" };
      return;
    }
    if (s.status !== "running" || this.#run === null) return;
    this.#run.controller.abort();
    void this.#deps.client.call("export.cancel", {});
  }

  /** Everything goes (the plugin is switched off), the last notice too: its buttons would call a gone backend. */
  close(): void {
    if (this.#run !== null) {
      this.#run.controller.abort();
      if (this.state.status === "running") void this.#deps.client.call("export.cancel", {});
    }
    this.#endRun();
    this.state = { status: "closed" };
    this.#deps.notices.dismissKey(KEY);
  }

  async #execute(
    entry: Entry,
    path: string,
    paper: {
      setup: PageSetup;
      page: PageBox;
      capabilities: PrintCapabilities;
    } | null,
  ): Promise<void> {
    const controller = new AbortController();
    const back = this.state;
    this.state = { status: "running", entry, path };
    this.#run = {
      controller,
      timer: setTimeout(() => this.#showProgress(entry), this.#deps.progressAfterMs),
    };
    const done = (text: string | null, ...actions: NoticeAction[]): void => {
      if (this.#run?.controller !== controller) return;
      this.#endRun();
      this.state = { status: "closed" };
      if (text !== null) this.#deps.notices.notifyKeyed(KEY, text, ...actions);
    };

    let outcome: ExportOutcome;
    try {
      outcome = await this.#deps.build(path, {
        target: paper === null ? "html" : "paper",
        ...(paper === null ? {} : { page: paper.page }),
        pageNumbers: paper !== null && paper.setup.pageNumbers && paper.capabilities.pageNumbers,
        nativeMargins: paper !== null && paper.capabilities.nativeMargins === true,
        env: this.#deps.env(),
        words: words(),
        signal: controller.signal,
      });
    } catch {
      done(controller.signal.aborted ? t("export.canceled") : t("export.buildFailed"));
      return;
    }
    switch (outcome.status) {
      case "cancelled":
        return done(t("export.canceled"));
      case "failed":
        return done(apiErrorText(outcome.error));
      case "notMarkdown":
        return done(t("export.notMarkdown"));
      case "done":
        break;
      default:
        return outcome satisfies never;
    }
    if (controller.signal.aborted) return done(t("export.canceled"));

    const { html, notes } = outcome;
    if (entry === "print") {
      if (paper === null) return done(null);
      const printed = await this.#deps.client.call("export.print", { html, setup: paper.setup });
      // a print stopped by Cancel ends quietly: whatever it answers is no failure to read
      if (!printed.ok && !controller.signal.aborted) {
        return done(failureText(entry, path, printed.error));
      }
      return done(null);
    }
    const saved =
      paper === null
        ? await this.#deps.client.call("export.saveHtml", { html, source: path })
        : await this.#deps.client.call("export.savePdf", {
            html,
            source: path,
            setup: paper.setup,
          });
    if (this.#run?.controller !== controller) return;
    if (!saved.ok) {
      return done(
        controller.signal.aborted ? t("export.canceled") : failureText(entry, path, saved.error),
      );
    }
    // a file written before Cancel arrived is on disk: it is reported Saved, never Canceled
    if (saved.value.outcome === "cancelled") {
      if (controller.signal.aborted) return done(t("export.canceled"));
      // the person closed the Save dialog: back to the page setup they came from
      done(null);
      if (back.status === "setup") this.state = { ...back, busy: false };
      return;
    }
    const left = notes.length > 0 ? ` ${tn("export.leftOut", notes.length)}` : "";
    done(
      `${t("export.saved", { name: fileName(saved.value.path) })}${left}`,
      { label: t("export.open"), run: () => this.#after("export.open") },
      { label: t("export.reveal"), run: () => this.#after("export.reveal") },
    );
  }

  async #after(method: "export.open" | "export.reveal"): Promise<void> {
    const result = await this.#deps.client.call(method, {});
    if (!result.ok) this.#deps.notices.notifyKeyed(KEY, apiErrorText(result.error));
  }

  /** A run that takes a while says so, with its Cancel (it stays until the run ends). */
  #showProgress(entry: Entry): void {
    if (this.#run === null) return;
    this.#run.timer = null;
    this.#progress = this.#deps.notices.notifyLasting({
      text: t(progressKey(entry)),
      actions: [{ label: t("common.cancel"), run: () => this.cancel() }],
      onGone: () => {
        this.#progress = null;
      },
    });
  }

  #endRun(): void {
    if (this.#run?.timer != null) clearTimeout(this.#run.timer);
    this.#run = null;
    if (this.#progress !== null) {
      const id = this.#progress;
      this.#progress = null;
      this.#deps.notices.dismiss(id);
    }
  }
}

/** The flow of this window. */
export const exporter = new ExportStore({
  client: pluginClient,
  build: buildWithPage,
  notices: windowStore,
  unsaved: (path) => editing.path === path && editing.unsaved,
  env: () => documents.enhanceEnv,
  region: () => navigator.language,
  progressAfterMs: 1000,
});

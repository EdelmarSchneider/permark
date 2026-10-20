// The converter dialog's state: one job at a time (closed -> working -> ready | refused | cancelled), the target the
// person looks at, and what an action is doing. Every call goes to the backend half; an answer for an older job is
// ignored. Secret values leave only after the person confirms.

import type { Analysis, ConvertSource } from "../../api";
import { pluginClient, type PluginClient } from "../../core/plugins/client";
import { sourceOf } from "./model";

/** Where the data comes from: a file, or a text of the page (saved next to the document it came from). */
export type Origin =
  | { kind: "file"; path: string }
  | { kind: "text"; text: string; language: string | null; beside: string | null };

export type Act = "copy" | "save" | "open";

export type Action =
  | { kind: "idle" }
  | { kind: "confirm"; act: Act }
  | { kind: "busy"; act: Act }
  | { kind: "done"; act: Act; path: string | null }
  | { kind: "failed"; act: Act };

export type ConvertState =
  | { status: "closed" }
  | { status: "working"; job: number; origin: Origin; from: string | null }
  | {
      status: "ready";
      job: number;
      origin: Origin;
      from: string | null;
      analysis: Analysis;
      selected: string;
      action: Action;
    }
  | {
      status: "refused";
      job: number;
      origin: Origin;
      from: string | null;
      reason: string;
      line: number;
      detail: string;
    }
  | { status: "cancelled"; job: number; origin: Origin; from: string | null };

function sourceArg(origin: Origin): ConvertSource {
  return origin.kind === "file"
    ? { kind: "file", path: origin.path }
    : { kind: "text", text: origin.text };
}

/** The target shown first: the best one, else the first that fits, else the first. */
function firstTarget(analysis: Analysis): string {
  return (
    analysis.targets.find((t) => t.best)?.id ??
    analysis.targets.find((t) => t.fit !== "no")?.id ??
    analysis.targets[0]?.id ??
    "toon"
  );
}

export class ConvertStore {
  state = $state.raw<ConvertState>({ status: "closed" });
  readonly #client: PluginClient;
  /** Opens a result in the window (a saved file). */
  #open: (path: string) => void = () => {};
  #clipboard: (text: string) => Promise<void> = (text) => navigator.clipboard.writeText(text);
  #next = 1;

  constructor(client: PluginClient = pluginClient) {
    this.#client = client;
  }

  bind(open: (path: string) => void, clipboard?: (text: string) => Promise<void>): void {
    this.#open = open;
    if (clipboard) this.#clipboard = clipboard;
  }

  /** Opens the dialog on a source (`from`: the format the person or the place says it is). */
  open(origin: Origin, from: string | null = null): void {
    const guess =
      from ??
      (origin.kind === "file"
        ? sourceOf(origin.path)
        : origin.language === null
          ? null
          : sourceOf(origin.language));
    void this.#analyze(origin, guess);
  }

  /** Reads the source again as another format. */
  readAs(from: string): void {
    const s = this.state;
    if (s.status === "closed") return;
    void this.#analyze(s.origin, from);
  }

  async #analyze(origin: Origin, from: string | null): Promise<void> {
    this.#stopRunning();
    this.#release();
    const job = this.#next++;
    this.state = { status: "working", job, origin, from };
    const result = await this.#client.call("convert.analyze", {
      job,
      source: sourceArg(origin),
      ...(from === null ? {} : { from }),
    });
    const now = this.state;
    if (now.status !== "working" || now.job !== job) return;
    if (!result.ok) {
      this.state = { status: "refused", job, origin, from, reason: "failed", line: 0, detail: "" };
      return;
    }
    const answer = result.value;
    switch (answer.status) {
      case "ready":
        this.state = {
          status: "ready",
          job,
          origin,
          from: answer.analysis.from,
          analysis: answer.analysis,
          selected: firstTarget(answer.analysis),
          action: { kind: "idle" },
        };
        break;
      case "refused":
        this.state = {
          status: "refused",
          job,
          origin,
          from,
          reason: answer.reason,
          line: answer.line,
          detail: answer.detail,
        };
        break;
      case "cancelled":
        this.state = { status: "cancelled", job, origin, from };
        break;
      default:
        answer satisfies never;
    }
  }

  /** Stops the analysis that runs (the backend stops at its next check). */
  cancel(): void {
    const s = this.state;
    if (s.status !== "working") return;
    void this.#client.call("convert.cancel", { job: s.job });
    this.state = { status: "cancelled", job: s.job, origin: s.origin, from: s.from };
  }

  select(target: string): void {
    const s = this.state;
    if (s.status !== "ready") return;
    this.state = { ...s, selected: target, action: { kind: "idle" } };
  }

  /** Copy | Save | Open the selected target; with secret values the person confirms first. */
  async act(act: Act, confirmed = false): Promise<void> {
    const s = this.state;
    if (s.status !== "ready") return;
    if (s.analysis.secrets > 0 && !confirmed) {
      this.state = { ...s, action: { kind: "confirm", act } };
      return;
    }
    this.state = { ...s, action: { kind: "busy", act } };
    const done = (action: Action): void => {
      const now = this.state;
      if (now.status === "ready" && now.job === s.job) this.state = { ...now, action };
    };
    const args = { job: s.job, target: s.selected, confirmed: s.analysis.secrets > 0 };
    if (act === "copy") {
      const result = await this.#client.call("convert.result", args);
      if (!result.ok) return done({ kind: "failed", act });
      try {
        await this.#clipboard(result.value);
        done({ kind: "done", act, path: null });
      } catch {
        done({ kind: "failed", act });
      }
      return;
    }
    const beside = s.origin.kind === "text" ? s.origin.beside : null;
    const result = await this.#client.call("convert.save", {
      ...args,
      ...(beside === null ? {} : { beside }),
    });
    if (!result.ok) return done({ kind: "failed", act });
    done({ kind: "done", act, path: result.value });
    if (act === "open") {
      this.close();
      this.#open(result.value);
    }
  }

  /** Back from the confirmation without doing anything. */
  dismiss(): void {
    const s = this.state;
    if (s.status === "ready") this.state = { ...s, action: { kind: "idle" } };
  }

  close(): void {
    this.#stopRunning();
    this.#release();
    this.state = { status: "closed" };
  }

  #stopRunning(): void {
    const s = this.state;
    if (s.status === "working") void this.#client.call("convert.cancel", { job: s.job });
  }

  /** The backend lets go of what it kept for the job (the data, the outputs). */
  #release(): void {
    const s = this.state;
    if (s.status === "ready") void this.#client.call("convert.close", { job: s.job });
  }
}

export const converter = new ConvertStore();

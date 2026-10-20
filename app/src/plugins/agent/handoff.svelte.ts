// The hand-off as the window runs it: ask the backend what would go (nothing new -> a quiet note, no dialog), let the
// person pick sessions (each change asks again: sizes and what fits), confirm -> the backend freezes the package and
// sets the checkpoint, the window copies one line (or the full list) and says how many actions went.

import type { Result } from "../../api";
import { t, tn } from "../../core/i18n";
import { PluginClient, pluginClient } from "../../core/plugins/client";
import { windowStore } from "../../core/window.svelte";
import { canConfirm, toggled, type Ready } from "./handoff";
import { agent } from "./store.svelte";

export type HandOffState =
  | { status: "closed" }
  | { status: "loading" }
  | { status: "open"; preview: Ready; full: boolean; phase: "idle" | "busy" | "failed" };

export type Copy = (text: string) => Promise<void>;
export type Note = (text: string) => void;

export class HandOffStore {
  state = $state.raw<HandOffState>({ status: "closed" });
  readonly #client: PluginClient;
  readonly #copy: Copy;
  readonly #note: Note;
  readonly #connected: () => boolean;

  constructor(
    client: PluginClient = pluginClient,
    copy: Copy = (text) => navigator.clipboard.writeText(text),
    note: Note = (text) => windowStore.notifyNote(text),
    connected: () => boolean = () => agent.enabled,
  ) {
    this.#client = client;
    this.#copy = copy;
    this.#note = note;
    this.#connected = connected;
  }

  /** Opens the dialog with the default scope; nothing new since the last hand-off -> a note only. */
  async open(): Promise<void> {
    if (this.state.status !== "closed") return;
    this.state = { status: "loading" };
    const result = await this.#client.call("agent.handOffPreview", {});
    if (this.state.status !== "loading") return;
    if (!result.ok) {
      this.state = { status: "closed" };
      this.#note(t("agent.handoff.failed"));
      return;
    }
    if (result.value.state === "nothing") {
      this.state = { status: "closed" };
      this.#note(t("agent.handoff.nothing"));
      return;
    }
    // without the connection the agent cannot ask PerMark: the full list is what helps
    this.state = {
      status: "open",
      preview: result.value,
      full: !this.#connected(),
      phase: "idle",
    };
  }

  /** Checks | unchecks a session; the preview is asked again (sizes, what fits). */
  async toggle(id: number): Promise<void> {
    const s = this.state;
    if (s.status !== "open" || s.phase === "busy") return;
    const checked = toggled(s.preview, id);
    this.state = { ...s, phase: "busy" };
    const result: Result<Ready | { state: "nothing" }> = await this.#client.call(
      "agent.handOffPreview",
      { checked },
    );
    const now = this.state;
    if (now.status !== "open") return;
    if (result.ok && result.value.state === "ready") {
      this.state = { ...now, preview: result.value, phase: "idle" };
    } else {
      this.state = { ...now, phase: "failed" };
    }
  }

  setFull(full: boolean): void {
    const s = this.state;
    if (s.status === "open") this.state = { ...s, full };
  }

  /** Hands off what the dialog shows: checkpoint + frozen package, then the copy. */
  async confirm(): Promise<void> {
    const s = this.state;
    if (s.status !== "open" || s.phase === "busy" || !canConfirm(s.preview)) return;
    this.state = { ...s, phase: "busy" };
    const checked = s.preview.sessions.filter((x) => x.checked).map((x) => x.id);
    const result = await this.#client.call("agent.handOffConfirm", {
      upto: s.preview.upto,
      checked,
    });
    if (!result.ok) {
      const now = this.state;
      if (now.status === "open") this.state = { ...now, phase: "failed" };
      return;
    }
    this.state = { status: "closed" };
    try {
      await this.#copy(s.full ? result.value.full : result.value.line);
      this.#note(tn("agent.handoff.done", result.value.rows));
    } catch {
      this.#note(t("agent.handoff.copyFailed"));
    }
  }

  close(): void {
    this.state = { status: "closed" };
  }
}

export const handOff = new HandOffStore();

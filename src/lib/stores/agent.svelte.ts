// The agent connection as the window sees it (docs/design/v0.4.md): the switch, the state the status bar shows, and
// the message that tells an agent what the person is looking at. The message is built from the reading view or the
// editor (`agentContext.ts`) and sent to the backend a moment after anything it depends on changes.

import { api } from "../api";
import {
  buildContext,
  sameContext,
  type Heading,
  type SelectedText,
  type View,
} from "../agentContext";
import { bridge } from "../bridge";
import { errorMessage } from "../errors";
import { t } from "../i18n";
import type { AgentContext, AgentStatus } from "../types";
import { docs } from "./docs.svelte";
import { edit } from "./edit.svelte";
import { outline } from "./outline.svelte";
import { toast } from "./toast.svelte";
import { vault } from "./vault.svelte";

/** The message is sent this long after the last change. */
export const PUBLISH_DELAY_MS = 300;
/** How long the status bar item pulses after a request. */
const PULSE_MS = 900;

const OFF: AgentStatus = {
  enabled: false,
  listening: false,
  connections: 0,
  recent: [],
  error: null,
  program: null,
};

class AgentStore {
  status = $state.raw<AgentStatus>(OFF);
  /** The item pulses: an agent just asked something. */
  pulsing = $state(false);
  /** The switch is being changed. */
  busy = $state(false);

  private timer: ReturnType<typeof setTimeout> | null = null;
  private pulseTimer: ReturnType<typeof setTimeout> | null = null;
  private lastSent: AgentContext | null = null;
  /** When the latest request seen was made; null until a status has been seen, so the first one is a starting point, not news. */
  private lastRequestAt: number | null = null;
  /** Bumped when the project changes, so a message still being built for the old one is dropped. */
  private generation = 0;

  /** On for the open project, and the project is one (a single file has no agent connection). */
  get enabled(): boolean {
    return this.status.enabled && !vault.isSimple;
  }

  /** An agent is connected: a `trailmark mcp` that is running, or a hook that is asking now. */
  get connected(): boolean {
    return this.status.listening && this.status.connections > 0;
  }

  /** A project was opened (or the window starts): the backend started the connection if it was on. */
  async onProjectOpened(): Promise<void> {
    this.generation++;
    this.lastSent = null;
    this.lastRequestAt = null;
    this.status = OFF;
    if (vault.isSimple) return;
    try {
      this.onStatus(await api.agentStatus());
    } catch {
      // the connection is a convenience; the window works without it
    }
    this.schedule();
  }

  /** The project is closing or being replaced. */
  reset(): void {
    this.generation++;
    this.clearTimer();
    this.lastSent = null;
    this.lastRequestAt = null;
    this.status = OFF;
    this.pulsing = false;
  }

  /** The backend says the connection changed (switched, an agent came or went, or one asked something). */
  onStatus(status: AgentStatus): void {
    const latest = status.recent[0]?.atMs ?? 0;
    const seen = this.lastRequestAt;
    // the first status after the window opened or the switch went on only says where things stand: a request already
    // in it was made before, whether or not there was one (a window without requests has `latest` 0, and the first
    // request that comes is news)
    if (seen !== null && latest > seen) this.pulse();
    // off forgets the starting point: the list of requests outlives the switch
    this.lastRequestAt = status.enabled ? Math.max(latest, seen ?? 0) : null;
    this.status = status;
  }

  private pulse(): void {
    this.pulsing = true;
    if (this.pulseTimer !== null) clearTimeout(this.pulseTimer);
    this.pulseTimer = setTimeout(() => {
      this.pulsing = false;
      this.pulseTimer = null;
    }, PULSE_MS);
  }

  // ---- the switch -----------------------------------------------------------------------------------

  /** Turns the connection on or off for the open project. The backend remembers the switch, in the app's own data. */
  async setEnabled(on: boolean): Promise<void> {
    if (this.busy || vault.isSimple) return;
    this.busy = true;
    try {
      this.onStatus(await api.setAgentConnection(on));
      this.lastSent = null;
      if (on) this.schedule(0);
    } catch (error) {
      toast.message(t("agent.failed", { message: errorMessage(error) }), "warn");
    } finally {
      this.busy = false;
    }
  }

  toggle(): Promise<void> {
    return this.setEnabled(!this.enabled);
  }

  // ---- what the person is looking at ------------------------------------------------------------------

  /**
   * Reads everything the message depends on, so an effect that calls this runs again when any of it changes: the
   * document, the mode, whether there are unsaved edits, the section being read, the outline, and the render.
   */
  track(): void {
    void docs.path;
    void docs.request;
    void edit.path;
    void edit.mode;
    void edit.dirty;
    void outline.current;
    void outline.model;
    void this.enabled;
    this.schedule();
  }

  /** Sends the message after a quiet moment (or at once with `0`). Does nothing while the connection is off. */
  schedule(delay = PUBLISH_DELAY_MS): void {
    if (!this.enabled) return;
    this.clearTimer();
    this.timer = setTimeout(() => void this.publish(), delay);
  }

  private clearTimer(): void {
    if (this.timer !== null) clearTimeout(this.timer);
    this.timer = null;
  }

  private async publish(): Promise<void> {
    this.timer = null;
    if (!this.enabled) return;
    const generation = this.generation;
    const context = await this.collect();
    if (generation !== this.generation || !this.enabled || sameContext(this.lastSent, context)) {
      return;
    }
    this.lastSent = context;
    try {
      await api.publishAgentContext(context);
    } catch {
      this.lastSent = null; // try again with the next change
    }
  }

  /** What the window shows now, as the message. */
  private async collect(): Promise<AgentContext> {
    const path = docs.path;
    const mode = edit.active ? edit.mode : "reading";
    let headings: readonly Heading[] = [];
    let lastLine = 1;
    let selection: SelectedText | null = null;
    if (path !== null) {
      try {
        if (!edit.active) {
          const snapshot = bridge.readingSnapshot?.();
          if (snapshot) ({ headings, lastLine, selection } = snapshot);
        } else if (bridge.editor) {
          // both editors give the text a save would write; its headings are read with the Source editor's parser
          const editor = bridge.editor;
          const text = editor.mode === "source" ? editor.getText() : editor.text();
          const { headingLinesOf } = await import("../editor/headingLines");
          ({ headings, lastLine } = headingLinesOf(text));
          selection = editor.selection?.() ?? null;
        }
      } catch {
        // the view is changing under us; the next change sends a better message
      }
    }
    const view: View = {
      document: path,
      mode,
      headings,
      lastLine,
      // the outline's section is the one being read; it belongs to this document only
      current: outline.path === path ? outline.current : null,
      selection,
      unsaved: edit.active && edit.dirty,
    };
    return buildContext(view);
  }
}

export const agent = new AgentStore();

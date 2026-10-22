// The agent connection as the window sees it: the switch and what the status bar shows (the backend's word), the
// context it publishes while the switch is on (debounced), and the places an agent asks to show. Dormant while off:
// nothing is published and nothing is shown but the settings section, its gear row and the once-only offer.

import type { AgentStatus, AgentWindowContext, PluginEvents, Result } from "../../api";
import { documents } from "../../core/doc/document.svelte";
import { editing } from "../../core/editing/session.svelte";
import { t } from "../../core/i18n";
import { PluginClient, pluginClient, workspaceRoot } from "../../core/plugins/client";
import { windowStore } from "../../core/window.svelte";
import { sameContext, sectionOf } from "./context";

/** Wait after the last change before the context is sent (ms). */
export const PUBLISH_DELAY_MS = 300;
/** Longest selection sent (the backend cuts again). */
const MAX_SELECTION = 4000;

export type Place = PluginEvents["agent:open"];

/** What the window shows now, as the agent connection takes it. Reactive. */
export type ContextPort = () => AgentWindowContext;

/** The page selection (reading view), kept by a `selectionchange` listener while the switch is on. */
type PageSelection = { text: string; lines: [number, number] | null } | null;

export class AgentStore {
  status = $state.raw<AgentStatus | null>(null);
  /** The running program's path (for the setup lines), asked once. */
  program = $state<string | null>(null);
  /** The popover of the status item is open. */
  popover = $state(false);
  #page = $state.raw<PageSelection>(null);
  #sent: AgentWindowContext | null = null;
  readonly #client: PluginClient;
  readonly #root: () => string | null;
  readonly #context: ContextPort;
  readonly #show: (place: Place) => void;

  constructor(
    client: PluginClient = pluginClient,
    root: () => string | null = workspaceRoot,
    context?: ContextPort,
    show?: (place: Place) => void,
  ) {
    this.#client = client;
    this.#root = root;
    this.#context = context ?? (() => this.#windowContext());
    this.#show = show ?? showPlace;
  }

  get enabled(): boolean {
    return this.status?.enabled === true;
  }

  get connected(): string[] {
    return this.status?.server.state === "listening" ? this.status.clients : [];
  }

  /** Follows the project of the window until the returned teardown runs. */
  start(): () => void {
    return $effect.root(() => {
      $effect(() => {
        if (this.#root() === null) {
          this.status = null;
          return;
        }
        let gone = false;
        const offs: (() => void)[] = [];
        const keep = (stop: () => void): void => {
          if (gone) stop();
          else offs.push(stop);
        };
        void this.#client.on("agent:status", (status) => (this.status = status)).then(keep);
        void this.#client.on("agent:open", (place) => this.#show(place)).then(keep);
        void this.#client.on("agent:ask", (ask) => this.#offer(ask.group)).then(keep);
        void this.#client.call("agent.status", {}).then((result) => {
          if (!gone && result.ok) this.status = result.value;
        });
        return () => {
          gone = true;
          for (const off of offs) off();
          this.#sent = null;
        };
      });
      // the page selection, only while the switch is on
      $effect(() => {
        if (!this.enabled) return;
        const read = (): void => {
          this.#page = pageSelection();
        };
        document.addEventListener("selectionchange", read);
        return () => {
          document.removeEventListener("selectionchange", read);
          this.#page = null;
        };
      });
      // publishing what the person looks at, a moment after it changes, only while the switch is on
      $effect(() => {
        if (!this.enabled) return;
        const context = this.#context();
        if (sameContext(this.#sent, context)) return;
        const timer = setTimeout(() => {
          this.#sent = context;
          void this.#client.call("agent.publish", { context });
        }, PUBLISH_DELAY_MS);
        return () => clearTimeout(timer);
      });
    });
  }

  #windowContext(): AgentWindowContext {
    const path = documents.path;
    const view = documents.view;
    const outline = view?.body.kind === "markdown" ? view.body.outline : [];
    const mode = editing.mode;
    const page = mode === "view" ? this.#page : null;
    const text = (mode === "view" ? (page?.text ?? "") : editing.selection).slice(0, MAX_SELECTION);
    return {
      doc: path,
      mode,
      section: path === null ? null : sectionOf(outline, documents.currentHeading),
      selection: text.trim() === "" ? null : { text, lines: page?.lines ?? null, cut: false },
      unsaved: editing.unsaved,
    };
  }

  /** Takes the new status; a refusal says so in a notice unless the caller shows it in place (`inline`). */
  #apply(result: Result<AgentStatus>, inline = false): boolean {
    if (result.ok) this.status = result.value;
    else if (!inline) windowStore.notifyNote(t("agent.notSaved"));
    return result.ok;
  }

  /** Turns the switch; false = refused (state unchanged). `inline`: the caller shows the refusal, no notice. */
  async setEnabled(enabled: boolean, inline = false): Promise<boolean> {
    const ok = this.#apply(await this.#client.call("agent.setEnabled", { enabled }), inline);
    if (!enabled) this.popover = false;
    return ok;
  }

  async setGroup(group: string, on: boolean): Promise<void> {
    this.#apply(await this.#client.call("agent.setGroup", { group, on }));
  }

  async setDefaults(): Promise<void> {
    const result = await this.#client.call("agent.setDefaults", {});
    windowStore.notifyNote(t(result.ok ? "agent.defaultsSaved" : "agent.notSaved"));
  }

  async loadProgram(): Promise<void> {
    const result = await this.#client.call("agent.setup", {});
    if (result.ok) this.program = result.value;
  }

  /** The first time an agent asks to mark comments done while that is off: offered once. */
  #offer(group: string): void {
    if (group !== "comments.resolve") return;
    windowStore.notifyAction(t("agent.ask.resolve"), {
      label: t("agent.ask.allow"),
      run: () => void this.setGroup(group, true),
    });
  }
}

/** The selection in the page (reading view): its text and lines; `null` outside the page. */
function pageSelection(): PageSelection {
  const selection = document.getSelection();
  if (selection === null || selection.isCollapsed) return null;
  const inPage = (node: Node | null): boolean => {
    const element = node instanceof Element ? node : (node?.parentElement ?? null);
    return element?.closest("[data-b]") != null;
  };
  if (!inPage(selection.anchorNode) || !inPage(selection.focusNode)) return null;
  const a = documents.referenceAt(selection.anchorNode)?.line ?? null;
  const b = documents.referenceAt(selection.focusNode)?.line ?? null;
  const lines: [number, number] | null =
    a !== null && b !== null ? [Math.min(a, b), Math.max(a, b)] : null;
  return { text: selection.toString(), lines };
}

/** Shows an agent's place: at once when the person reads, as a notice with a button while they edit. Never focuses. */
export function showPlace(place: Place): void {
  const go = (): void => documents.navigate(place.path, null, place.line);
  if (editing.mode !== "view") {
    windowStore.notifyAction(t("agent.open.notice", { path: place.path }), {
      label: t("agent.open.show"),
      run: go,
    });
    return;
  }
  go();
}

export const agent = new AgentStore();

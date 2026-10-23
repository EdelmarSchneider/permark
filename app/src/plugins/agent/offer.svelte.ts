// The once-only offer as the window keeps it: the answer of the shown project (read per project load), whether the
// strip shows, and the two answers. The switch turned on by any path while unasked counts as `setup`.

import { api, type Api } from "../../api";
import { agentFilesOf } from "../../core/conditions/agents";
import { conditions } from "../../core/conditions/context.svelte";
import { workspaceRoot } from "../../core/plugins/client";
import { ui } from "../../core/shell/ui.svelte";
import {
  OFFER_KEY,
  offerStateOf,
  shouldOfferAgent,
  type OfferAnswer,
  type OfferState,
} from "./offer";
import { agent, type AgentStore } from "./store.svelte";

/** The project session, as far as the offer uses it. */
export type SessionPort = Pick<Api, "projectSessionGet" | "projectSessionSet">;

/** The settings section of the agent connection (`settingsPage` id). */
export const AGENT_PAGE = "agent.page";

export class AgentOffer {
  state = $state.raw<OfferState>({ status: "loading" });
  readonly #session: SessionPort;
  readonly #root: () => string | null;
  readonly #agent: AgentStore;

  constructor(
    session: SessionPort = api,
    root: () => string | null = workspaceRoot,
    store: AgentStore = agent,
  ) {
    this.#session = session;
    this.#root = root;
    this.#agent = store;
  }

  /** The agent files the strip names (the first one), as the backend found them. */
  get files(): readonly string[] {
    return agentFilesOf(conditions.current);
  }

  /** The strip shows now. Reactive. */
  get shown(): boolean {
    return shouldOfferAgent({
      workspace: conditions.current.workspace,
      files: this.files,
      state: this.state,
      enabled: this.#agent.status === null ? null : this.#agent.enabled,
    });
  }

  /** Follows the window's project until the returned teardown runs. */
  start(): () => void {
    return $effect.root(() => {
      $effect(() => {
        const root = this.#root();
        this.state = { status: "loading" };
        if (root === null) return;
        let gone = false;
        void this.#session.projectSessionGet(OFFER_KEY).then((result) => {
          if (gone) return;
          this.state = result.ok ? offerStateOf(result.value) : { status: "unreadable" };
        });
        return () => {
          gone = true;
        };
      });
      // the person found the switch alone: the offer would only repeat what they did
      $effect(() => {
        if (this.#agent.enabled && this.state.status === "unasked") void this.answer("setup");
      });
    });
  }

  /** Remembers the answer for this project (for good); the strip goes at once. */
  async answer(answer: OfferAnswer): Promise<void> {
    this.state = { status: "answered", answer };
    await this.#session.projectSessionSet(OFFER_KEY, answer);
  }

  /** "Set up…": the settings section opens on its first control; nothing is turned on. */
  async setUp(): Promise<void> {
    ui.openSettingsPage(AGENT_PAGE, "content");
    await this.answer("setup");
  }

  /** "No thanks". */
  async decline(): Promise<void> {
    await this.answer("declined");
  }
}

export const offer = new AgentOffer();

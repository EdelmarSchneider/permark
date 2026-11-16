// The demo pages offer of the Welcome screen: "Explore the demo pages" copies the sample documents shipped with the app
// into the Documents folder and opens them as a project; "Open the demo pages" when the folder is there already (the
// Windows installer may have copied it). Shown until the person uses it or dismisses it (the backend remembers).

import { api as defaultApi, type Api } from "../../api";
import { apiErrorText } from "../errors";
import { t } from "../i18n";
import type { WindowStore } from "../window.svelte";

export type DemoCard =
  | { status: "hidden" }
  /** `problem`: why the last copy failed, in words (`null`: none). */
  | { status: "offered"; offer: "install" | "open"; problem: string | null }
  | { status: "working"; offer: "install" | "open" };

export class DemoOfferStore {
  card = $state.raw<DemoCard>({ status: "hidden" });
  readonly #api: Api;

  constructor(api: Api = defaultApi) {
    this.#api = api;
  }

  async load(): Promise<void> {
    const result = await this.#api.demoOffer();
    const offer = result.ok ? result.value : "hidden";
    this.card =
      offer === "hidden" ? { status: "hidden" } : { status: "offered", offer, problem: null };
  }

  /** Copies the pages into Documents when they are not there yet, then opens the folder as "Open folder…" does. */
  async use(store: WindowStore): Promise<void> {
    const card = this.card;
    if (card.status !== "offered") return;
    this.card = { status: "working", offer: card.offer };
    const result = await this.#api.demoInstall();
    if (!result.ok) {
      this.card = {
        status: "offered",
        offer: card.offer,
        problem: t("welcome.demo.failed", { message: apiErrorText(result.error) }),
      };
      return;
    }
    this.card = { status: "hidden" };
    await store.open(result.value);
  }

  async dismiss(): Promise<void> {
    if (this.card.status !== "offered") return;
    this.card = { status: "hidden" };
    await this.#api.demoDismiss();
  }
}

export const demoOffer = new DemoOfferStore();

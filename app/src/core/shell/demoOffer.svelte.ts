// The demo pages offer of the Welcome screen: "Explore the demo pages" copies the sample documents shipped with the app
// into the Documents folder and opens them as a project; "Open the demo pages" when the folder is there already (the
// Windows installer may have copied it). Shown until the person uses it or dismisses it (the backend remembers). The
// palette command and the quick settings entry open the pages the same way whenever this copy ships them.

import { api as defaultApi, type Api, type ApiError } from "../../api";
import { apiErrorText } from "../errors";
import { t } from "../i18n";
import type { WindowStore } from "../window.svelte";

export type DemoCard =
  | { status: "hidden" }
  /** `problem`: why the last copy failed, in words (`null`: none). */
  | { status: "offered"; offer: "install" | "open"; problem: string | null }
  | { status: "working"; offer: "install" | "open" };

/** One notice for a refused copy from the lasting way in (a second try takes its place). */
const NOTICE_KEY = "demo-pages";

function failedText(error: ApiError): string {
  return t("welcome.demo.failed", { message: apiErrorText(error) });
}

export class DemoOfferStore {
  card = $state.raw<DemoCard>({ status: "hidden" });
  /** The pages can be opened (folder in Documents | shipped with this copy), whatever became of the offer. */
  available = $state(false);
  readonly #api: Api;

  constructor(api: Api = defaultApi) {
    this.#api = api;
  }

  async load(): Promise<void> {
    const result = await this.#api.demoOffer();
    const offer = result.ok ? result.value.offer : "hidden";
    this.available = result.ok && result.value.available;
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
        problem: failedText(result.error),
      };
      return;
    }
    await this.#opened(store, result.value);
  }

  /** The palette command and the quick settings entry: the card's copy + open, a refusal told by a notice. */
  async open(store: WindowStore): Promise<void> {
    const result = await this.#api.demoInstall();
    if (!result.ok) {
      store.notifyKeyed(NOTICE_KEY, failedText(result.error));
      return;
    }
    await this.#opened(store, result.value);
  }

  /** The folder is in Documents and the offer counts as used: opened as a recent folder is (the `open_in` setting). */
  async #opened(store: WindowStore, path: string): Promise<void> {
    this.card = { status: "hidden" };
    await store.open(path);
  }

  async dismiss(): Promise<void> {
    if (this.card.status !== "offered") return;
    this.card = { status: "hidden" };
    await this.#api.demoDismiss();
  }
}

export const demoOffer = new DemoOfferStore();

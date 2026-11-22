// The default app, by the person's choice: the rows of Settings -> Integration and the first-run question. Where the
// system lets PerMark set it (macOS, an installed Linux package) the rows are checks; on Windows they say who opens the
// files now and hand the person to Windows' Default Apps page.

import {
  api as defaultApi,
  type Api,
  type ApiError,
  type DefaultAppView,
  type FileGroup,
} from "../../api";
import { apiErrorText } from "../errors";
import { t } from "../i18n";

export type Question =
  | { status: "closed" }
  /** The first-run question, with the groups ticked so far (Markdown at first). */
  | { status: "asking"; view: DefaultAppView; chosen: readonly FileGroup[] };

/** `detail` of a refusal to give a type back to "no choice" (mirror of Rust `desktop::NO_HANDLER_TO_RESTORE`). */
const NO_HANDLER_TO_RESTORE = "noHandlerToRestore";

/** A refused change in words: where the system cannot give a type back, where the person picks another app. */
function refusalText(error: ApiError): string {
  return error.kind === "backend" && error.error.detail === NO_HANDLER_TO_RESTORE
    ? t("defaultApp.chooseInFinder")
    : t("defaultApp.failed", { message: apiErrorText(error) });
}

export class DefaultAppStore {
  view = $state.raw<DefaultAppView | null>(null);
  question = $state.raw<Question>({ status: "closed" });
  /** A refusal of the last change, in words (`null`: none). */
  problem = $state<string | null>(null);
  readonly #api: Api;
  #asked = false;

  constructor(api: Api = defaultApi) {
    this.#api = api;
  }

  async load(): Promise<void> {
    const result = await this.#api.defaultAppGet();
    if (result.ok) this.view = result.value;
  }

  /** Asks once per run, in the first window (`main`), when the backend says the question is due. */
  async askIfDue(window: string): Promise<void> {
    if (this.#asked || window !== "main") return;
    this.#asked = true;
    await this.load();
    const view = this.view;
    if (view?.ask === true) {
      this.question = { status: "asking", view, chosen: ["markdown"] };
    }
  }

  toggleChosen(group: FileGroup, on: boolean): void {
    const question = this.question;
    if (question.status !== "asking") return;
    const chosen = question.chosen.filter((g) => g !== group);
    this.question = { ...question, chosen: on ? [...chosen, group] : chosen };
  }

  /** The person answered: the chosen groups become PerMark's (or Windows' page opens); `use = false` = "Not now". */
  async answer(use: boolean): Promise<void> {
    const question = this.question;
    if (question.status !== "asking") return;
    this.question = { status: "closed" };
    if (!use || question.chosen.length === 0) {
      await this.#api.defaultAppAsked();
      return;
    }
    switch (question.view.mode.mode) {
      case "handoff":
        await this.handoff();
        return;
      case "direct":
        for (const group of question.chosen) await this.set(group, true);
        return;
      case "unsupported":
        await this.#api.defaultAppAsked();
        return;
      default:
        question.view.mode satisfies never;
    }
  }

  async set(group: FileGroup, on: boolean): Promise<void> {
    const result = await this.#api.defaultAppSet(group, on);
    if (result.ok) {
      this.view = result.value;
      this.problem = null;
    } else {
      this.problem = refusalText(result.error);
      // what the system holds now (a refusal may come after some types changed)
      await this.load();
    }
  }

  async handoff(): Promise<void> {
    const result = await this.#api.defaultAppHandoff();
    this.problem = result.ok
      ? null
      : t("defaultApp.failed", { message: apiErrorText(result.error) });
  }
}

export const defaultApp = new DefaultAppStore();

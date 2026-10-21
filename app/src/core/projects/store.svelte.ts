// Projects inside the folder this view shows (`projects.md` §12): the backend's `ProjectsView`, re-read on
// `projects-changed`, and every action of the strip, Settings > Projects, the project node and its card. One store per
// view (the module instance); started while a project is shown, reset when it goes.

import type {
  AgentChoice,
  Api,
  ApiError,
  CardChange,
  CardDto,
  CardTarget,
  HiddenRule,
  LocateRefusal,
  ProjectPick,
  ProjectRow,
  ProjectsView,
  Result,
  Unlisten,
} from "../../api";
import { apiErrorText } from "../errors";
import { t, tn } from "../i18n";
import { membersOf, offeredPaths, ownerOf, readOnly, stripOf, type Strip } from "./model";

/** Wait after `projects-changed` before the view is read again (spec: 100 ms debounce). */
export const RELOAD_MS = 100;

export type NoticeAction = { label: string; run: () => void };

/** What the store needs from its window. */
export type ProjectsHost = {
  note(text: string): void;
  action(text: string, ...actions: NoticeAction[]): void;
  /** A failed action, in words. */
  fail(text: string): void;
  /** Settings > Projects opens (the page shows the section). */
  openSettings(): void;
  closeSettings(): void;
};

/** The detail card of a project node: where it hangs, how it was opened (hover: no focus move; menu: focus inside). */
export type CardOpen = { path: string; from: "hover" | "menu"; anchor: DOMRect };

/** Words for a failed project action: the project list changed under the save | anything else by kind. */
export function projectsErrorText(error: ApiError): string {
  if (
    error.kind === "backend" &&
    error.error.kind === "conflict" &&
    (error.error.subject === "projects" || error.error.subject === "card")
  ) {
    return t("projects.record.conflict");
  }
  return apiErrorText(error);
}

export class ProjectsStore {
  readonly #api: Api;
  readonly #host: ProjectsHost;

  /** The folder shown (`null`: no project). */
  root = $state<string | null>(null);
  view = $state.raw<ProjectsView | null>(null);
  /** A strip showed during this open: the code offer waits for the next open (one strip at a time). */
  askedThisOpen = $state(false);
  /** Settings > Projects in consent mode ("Choose…"): the paths checked; `null`: normal mode. */
  choosing = $state.raw<ReadonlySet<string> | null>(null);
  /** A row Settings opens expanded, focus on its first rule (node menu "Project settings…"); `n` makes asks distinct. */
  focusRow = $state.raw<{ key: string; n: number } | null>(null);
  /** The detail card shown (`null`: none). */
  card = $state.raw<CardOpen | null>(null);

  strip: Strip = $derived(stripOf(this.view));
  members: ReadonlyMap<string, ProjectRow> = $derived(membersOf(this.view));
  /** Settings > Projects exists here (a list, or >= 1 project found). */
  present = $derived(this.view?.present === true);
  readOnly = $derived(this.view !== null && readOnly(this.view));

  #timer: ReturnType<typeof setTimeout> | null = null;
  #generation = 0;
  #asks = 0;

  constructor(api: Api, host: ProjectsHost) {
    this.#api = api;
    this.#host = host;
  }

  // ---- lifecycle -------------------------------------------------------------------------------------------------

  /** Follows the projects of `root` (listens first, then reads: no change lost). Returns the teardown. */
  async start(root: string): Promise<Unlisten> {
    this.reset();
    const generation = this.#generation;
    this.root = root;
    const off = await this.#api.on("projects-changed", (event) => {
      if (event.root !== root || generation !== this.#generation) return;
      if (this.view !== null && event.seq <= this.view.seq) return;
      this.#schedule();
    });
    await this.reload();
    return () => {
      off();
      if (generation === this.#generation) this.reset();
    };
  }

  /** Back to nothing (no project shown). */
  reset(): void {
    this.#generation += 1;
    if (this.#timer !== null) clearTimeout(this.#timer);
    this.#timer = null;
    this.root = null;
    this.view = null;
    this.askedThisOpen = false;
    this.choosing = null;
    this.focusRow = null;
    this.card = null;
  }

  #schedule(): void {
    if (this.#timer !== null) clearTimeout(this.#timer);
    this.#timer = setTimeout(() => {
      this.#timer = null;
      void this.reload();
    }, RELOAD_MS);
  }

  /** Reads the view now (an older answer than the one shown is dropped). */
  async reload(): Promise<void> {
    const generation = this.#generation;
    const result = await this.#api.projectsView();
    if (generation !== this.#generation || !result.ok) return;
    if (this.view !== null && result.value.seq < this.view.seq) return;
    this.view = result.value;
    // a strip showed in this open: the code offer waits for the next one
    if (stripOf(this.view).kind !== "none") this.askedThisOpen = true;
    if (this.choosing !== null) {
      // a project that went away while choosing is no longer checked
      const offered = new Set(offeredPaths(this.view));
      this.choosing = new Set([...this.choosing].filter((p) => offered.has(p)));
    }
  }

  // ---- lookups ---------------------------------------------------------------------------------------------------

  /** The included project at exactly this folder (`null`: a plain folder). */
  memberAt(path: string): ProjectRow | null {
    if (this.members.size === 0) return null;
    return this.members.get(path.replaceAll("\\", "/").normalize("NFC").toLowerCase()) ?? null;
  }

  /** The included project that owns a path (`null`: the opened folder). */
  ownerOf(path: string | null): ProjectRow | null {
    return ownerOf(this.members, path);
  }

  rowByKey(key: string): ProjectRow | null {
    return this.view?.rows.find((r) => r.key === key) ?? null;
  }

  // ---- the strip -------------------------------------------------------------------------------------------------

  /** "Include all" | "Include": every offered project; a notice with Undo. */
  async includeAll(): Promise<void> {
    await this.#include({ kind: "all" });
  }

  /** "No thanks": the first question declines for this folder; later ones decline the projects asked about. */
  async declineOffered(): Promise<void> {
    await this.#run(this.#api.projectsDecline({ kind: "all" }));
  }

  /** The move question: `same` = the found project is the missing one. */
  async answerMove(same: boolean): Promise<void> {
    const ask = this.view?.ask;
    if (!ask) return;
    await this.#run(this.#api.projectsAnswerMove(ask.uid, ask.candidate, same));
  }

  // ---- Choose… (consent mode of Settings > Projects) -------------------------------------------------------------

  /** Opens Settings > Projects with every offered project checked; nothing is saved until "Include checked". */
  beginChoose(): void {
    this.choosing = new Set(offeredPaths(this.view));
    this.#host.openSettings();
  }

  setChosen(path: string, on: boolean): void {
    if (this.choosing === null) return;
    const next = new Set(this.choosing);
    if (on) next.add(path);
    else next.delete(path);
    this.choosing = next;
  }

  /** "Include checked": checked ones included, the others left out (they are not asked again); the page closes. */
  async confirmChoose(): Promise<void> {
    const checked = this.choosing;
    if (checked === null) return;
    const unchecked = offeredPaths(this.view).filter((p) => !checked.has(p));
    this.choosing = null;
    this.#host.closeSettings();
    if (checked.size > 0) await this.#include({ kind: "paths", paths: [...checked] });
    if (unchecked.length > 0) {
      await this.#run(this.#api.projectsDecline({ kind: "paths", paths: unchecked }));
    }
  }

  /** "Cancel" | Esc: nothing saved; the strip still asks. */
  cancelChoose(): void {
    if (this.choosing === null) return;
    this.choosing = null;
    this.#host.closeSettings();
  }

  // ---- Settings > Projects ---------------------------------------------------------------------------------------

  /** Opens Settings > Projects; with a project: its row expanded, focus on its first rule. */
  openSettings(row: ProjectRow | null = null): void {
    this.choosing = null;
    if (row !== null) this.focusRow = { key: row.key, n: ++this.#asks };
    this.#host.openSettings();
  }

  /** The checkbox of a row: include it | leave it out. */
  async setIncluded(row: ProjectRow, on: boolean): Promise<void> {
    if (on) {
      if (row.state === "offered" || row.state === "declined") {
        await this.#include({ kind: "paths", paths: [row.path] });
      }
    } else if (row.state === "included" && row.uid !== null) {
      await this.#run(this.#api.projectsLeaveOut([row.uid]));
    }
  }

  /** "Include shown": the not-included rows the filter shows. */
  async includeShown(rows: readonly ProjectRow[]): Promise<void> {
    const paths = rows
      .filter((r) => r.state === "offered" || r.state === "declined")
      .map((r) => r.path);
    if (paths.length > 0) await this.#include({ kind: "paths", paths });
  }

  /** "Leave out shown": the included rows the filter shows. */
  async leaveOutShown(rows: readonly ProjectRow[]): Promise<void> {
    const uids = rows.flatMap((r) => (r.state === "included" && r.uid !== null ? [r.uid] : []));
    if (uids.length > 0) await this.#run(this.#api.projectsLeaveOut(uids));
  }

  async setHidden(row: ProjectRow, rule: HiddenRule): Promise<void> {
    if (row.uid !== null) await this.#run(this.#api.projectsSetHidden(row.uid, rule));
  }

  async setAgent(row: ProjectRow, choice: AgentChoice): Promise<void> {
    if (row.uid !== null) await this.#run(this.#api.projectsSetAgent(row.uid, choice));
  }

  /** Locate… of a missing project: the folder picker, then the backend's checks. Returns the refusal in words
   *  (`null`: placed, or the picker was closed). */
  async locate(row: ProjectRow, title: string): Promise<string | null> {
    if (row.uid === null) return null;
    const picked = await this.#api.pickFolder(title);
    if (picked === null) return null;
    const result = await this.#api.projectsLocate(row.uid, picked);
    if (!result.ok) {
      this.#host.fail(projectsErrorText(result.error));
      return null;
    }
    await this.reload();
    return result.value === null ? null : this.#refusalText(result.value);
  }

  #refusalText(refusal: LocateRefusal): string {
    const root = this.view?.rootCard.name ?? "";
    switch (refusal) {
      case "outside":
        return t("projects.locate.outside", { root });
      case "notProject":
        return t("projects.locate.notProject");
      case "taken":
        return t("projects.locate.taken");
      case "link":
        return t("projects.locate.link");
      default:
        return refusal satisfies never;
    }
  }

  /** "Remove from list" of a missing project. */
  async remove(row: ProjectRow): Promise<void> {
    if (row.uid !== null) await this.#run(this.#api.projectsRemove([row.uid]));
  }

  /** One card field; the card as saved, or `null` (the failure was said). */
  async setCard(target: CardTarget, change: CardChange): Promise<CardDto | null> {
    const result = await this.#api.projectsCardSet(target, change);
    if (!result.ok) {
      this.#host.fail(projectsErrorText(result.error));
      return null;
    }
    await this.reload();
    return result.value;
  }

  // ---- the detail card -------------------------------------------------------------------------------------------

  openCard(open: CardOpen): void {
    this.card = open;
  }

  closeCard(): void {
    this.card = null;
  }

  // ---- shared ----------------------------------------------------------------------------------------------------

  async #include(pick: ProjectPick): Promise<void> {
    const result = await this.#api.projectsInclude(pick);
    if (!result.ok) {
      this.#host.fail(projectsErrorText(result.error));
      return;
    }
    await this.reload();
    const { included, undo } = result.value;
    if (included === 0) return;
    const text = tn("projects.included", included);
    if (undo === null) this.#host.note(text);
    else {
      this.#host.action(text, {
        label: t("fileOps.undo"),
        run: () => void this.#undo(undo),
      });
    }
  }

  async #undo(token: number): Promise<void> {
    const result = await this.#api.projectsUndo(token);
    if (!result.ok) {
      this.#host.fail(projectsErrorText(result.error));
      return;
    }
    await this.reload();
    if (result.value.permarkKept) this.#host.note(t("projects.undoKept"));
  }

  async #run(call: Promise<Result<null>>): Promise<void> {
    const result = await call;
    if (!result.ok) {
      this.#host.fail(projectsErrorText(result.error));
      return;
    }
    await this.reload();
  }
}

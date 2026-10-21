// Projects inside the folder this view shows (`projects.md` §12): the backend's `ProjectsView`, re-read on
// `projects-changed`, and every action of the strip, Settings > Projects inside, the project node and its card. One
// store per view (the module instance); started while a project is shown, reset when it goes.

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
import {
  folderName,
  membersOf,
  offeredPaths,
  ownerOf,
  readOnly,
  stripOf,
  type Strip,
} from "./model";

/** Wait after `projects-changed` before the view is read again (spec: 100 ms debounce). */
export const RELOAD_MS = 100;
/** A projects notice closes by itself after this long (paused while the pointer | focus is on it). */
export const NOTICE_MS = 8000;
/** Pause before an announcement is set (the region was emptied first: the same words twice are said twice). */
const SAY_MS = 50;

export type NoticeAction = { label: string; run: () => void };
/** How a notice went: its button ran, it was dismissed | timed out | pushed out by newer ones. */
export type NoticeGone = "action" | "dismissed" | "timeout" | "pushedOut";
export type TimedNotice = {
  text: string;
  actions: readonly NoticeAction[];
  closeAfterMs: number;
  onGone?: (how: NoticeGone) => void;
};

/** What the store needs from its window. */
export type ProjectsHost = {
  /** A quiet notice (closes by itself; announced politely). */
  notice(notice: TimedNotice): void;
  /** A failed action, in words. */
  fail(text: string): void;
  /** Settings > Projects inside opens (the page shows the section). */
  openSettings(): void;
  closeSettings(): void;
  /** Whether the settings page shows now. */
  settingsShown(): boolean;
  /** Focus lost by an action (its button went away) goes to the next sensible place: the strip | the file list. */
  refocus(): void;
};

/** What Settings > Projects inside does when it shows: a row opened + focused (its first rule | its first action), or
 *  the problem of the list focused. `n` makes asks distinct. */
export type SettingsAsk = { kind: "row"; key: string; n: number } | { kind: "problem"; n: number };

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
  /** The view as the backend sent it. */
  served = $state.raw<ProjectsView | null>(null);
  /** Missing projects "Remove from list" took away, kept until their notice goes (Undo = nothing happened). */
  removing = $state.raw<ReadonlySet<string>>(new Set());
  /** The view shown: without the rows a removal waits to take away. */
  view: ProjectsView | null = $derived(withoutRows(this.served, this.removing));
  /** A strip showed during this open: the code offer waits for the next open (one strip at a time). */
  askedThisOpen = $state(false);
  /** Settings > Projects inside in consent mode ("Choose…"): the paths checked; `null`: normal mode. */
  choosing = $state.raw<ReadonlySet<string> | null>(null);
  /** What Settings > Projects inside does when it shows (node menu "Project settings…", a Changes row, the broken
   *  list strip, an undone removal). */
  ask = $state.raw<SettingsAsk | null>(null);
  /** Polite words for screen readers (a row moved group, an include undone); empty between messages. */
  announcement = $state("");
  /** The detail card shown (`null`: none). */
  card = $state.raw<CardOpen | null>(null);

  strip: Strip = $derived(stripOf(this.view));
  members: ReadonlyMap<string, ProjectRow> = $derived(membersOf(this.view));
  /** Settings > Projects inside exists here (a list, or >= 1 project found). */
  present = $derived(this.view?.present === true);
  readOnly = $derived(this.view !== null && readOnly(this.view));

  #timer: ReturnType<typeof setTimeout> | null = null;
  #sayTimer: ReturnType<typeof setTimeout> | null = null;
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
      if (this.served !== null && event.seq <= this.served.seq) return;
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
    if (this.#sayTimer !== null) clearTimeout(this.#sayTimer);
    this.#timer = null;
    this.#sayTimer = null;
    this.root = null;
    this.served = null;
    this.askedThisOpen = false;
    this.choosing = null;
    this.ask = null;
    this.card = null;
    this.announcement = "";
    // a removal still waiting is dropped: the entry stays (nothing lost; the person can remove it again)
    this.removing = new Set();
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
    if (this.served !== null && result.value.seq < this.served.seq) return;
    this.served = result.value;
    // a strip showed in this open: the code offer waits for the next one
    if (stripOf(this.served).kind !== "none") this.askedThisOpen = true;
    if (this.choosing !== null) {
      // a project that went away while choosing is no longer checked
      const offered = new Set(offeredPaths(this.served));
      this.choosing = new Set([...this.choosing].filter((p) => offered.has(p)));
    }
  }

  /** Says `text` to screen readers (the region is emptied first: the same words twice are said twice). */
  announce(text: string): void {
    if (this.#sayTimer !== null) clearTimeout(this.#sayTimer);
    this.announcement = "";
    const generation = this.#generation;
    this.#sayTimer = setTimeout(() => {
      this.#sayTimer = null;
      if (generation === this.#generation) this.announcement = text;
    }, SAY_MS);
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

  /** The row of a project path, any state (`null`: not in the view). */
  rowAt(path: string): ProjectRow | null {
    const key = path.replaceAll("\\", "/").normalize("NFC").toLowerCase();
    return this.view?.rows.find((r) => r.key === key) ?? null;
  }

  // ---- the strip -------------------------------------------------------------------------------------------------

  /** "Include all" | "Include": every offered project; a notice with Undo. */
  async includeAll(): Promise<void> {
    await this.#include({ kind: "all" });
  }

  /** "No thanks": the first question declines for this folder; later ones decline the projects asked about. A notice
   *  says where to change the mind. */
  async declineOffered(): Promise<void> {
    if (await this.#run(this.#api.projectsDecline({ kind: "all" }))) {
      this.#note(t("projects.declined"));
    }
  }

  /** The move question: `same` = the found project is the missing one. */
  async answerMove(same: boolean): Promise<void> {
    const ask = this.view?.ask;
    if (!ask) return;
    await this.#run(this.#api.projectsAnswerMove(ask.uid, ask.candidate, same));
  }

  // ---- Choose… (consent mode of Settings > Projects inside) ------------------------------------------------------

  /** Opens Settings > Projects inside with every offered project checked; nothing is saved until "Include checked". */
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

  // ---- Settings > Projects inside --------------------------------------------------------------------------------

  /** Opens Settings > Projects inside; with a project: its row expanded, focus on its first rule (a missing one: its
   *  first action). */
  openSettings(row: ProjectRow | null = null): void {
    this.choosing = null;
    if (row !== null) this.ask = { kind: "row", key: row.key, n: ++this.#asks };
    this.#host.openSettings();
  }

  /** The broken list strip's "Show the problem": Settings > Projects inside, focus on the problem. */
  showProblem(): void {
    this.choosing = null;
    this.ask = { kind: "problem", n: ++this.#asks };
    this.#host.openSettings();
  }

  /** The checkbox of a row: include it | leave it out. The row moves to the other group; the screen reader hears
   *  where (the page moves the focus). Returns whether it moved. */
  async setIncluded(row: ProjectRow, on: boolean): Promise<boolean> {
    let moved = false;
    if (on) {
      if (row.state === "offered" || row.state === "declined") {
        // the page shows the move and says it: no notice
        moved = await this.#include({ kind: "paths", paths: [row.path] }, false);
      }
    } else if (row.state === "included" && row.uid !== null) {
      moved = await this.#run(this.#api.projectsLeaveOut([row.uid]));
    }
    if (moved) {
      const name = folderName(row.path);
      this.announce(t(on ? "projects.announce.included" : "projects.announce.leftOut", { name }));
    }
    return moved;
  }

  /** "Include the projects listed": the not-included rows the filter shows. */
  async includeShown(rows: readonly ProjectRow[]): Promise<void> {
    const paths = rows
      .filter((r) => r.state === "offered" || r.state === "declined")
      .map((r) => r.path);
    if (paths.length > 0) await this.#include({ kind: "paths", paths });
  }

  /** "Leave out the projects listed": the included rows the filter shows. */
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

  /** "Remove from list" of a missing project: the row goes at once, a notice offers Undo; the entry leaves the list
   *  when the notice goes (Undo: it never left; its id and rules stay as they were). Closed window | other folder
   *  before that: the entry stays. */
  remove(row: ProjectRow): void {
    const uid = row.uid;
    if (uid === null || this.removing.has(uid)) return;
    const generation = this.#generation;
    this.removing = new Set([...this.removing, uid]);
    const name = folderName(row.path);
    this.#host.notice({
      text: t("projects.removed", { name }),
      actions: [
        { label: t("fileOps.undo"), run: () => this.#keep(uid, row.key, name, generation) },
      ],
      closeAfterMs: NOTICE_MS,
      onGone: (how) => {
        if (how !== "action") void this.#commitRemove(uid, generation);
      },
    });
  }

  /** Undo of a removal: the row is back where it was (focus on it when the page shows). */
  #keep(uid: string, key: string, name: string, generation: number): void {
    if (generation !== this.#generation || !this.removing.has(uid)) return;
    this.#forget(uid);
    this.announce(t("projects.announce.kept", { name }));
    if (this.#host.settingsShown()) this.ask = { kind: "row", key, n: ++this.#asks };
    else this.#host.refocus();
  }

  async #commitRemove(uid: string, generation: number): Promise<void> {
    if (generation !== this.#generation || !this.removing.has(uid)) return;
    const result = await this.#api.projectsRemove([uid]);
    if (generation !== this.#generation) return;
    if (result.ok) await this.reload();
    else this.#host.fail(projectsErrorText(result.error));
    if (generation === this.#generation) this.#forget(uid);
  }

  #forget(uid: string): void {
    const next = new Set(this.removing);
    next.delete(uid);
    this.removing = next;
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

  /** Includes; a notice (with Undo when the backend gives one) unless the page shows the move itself. Returns whether
   *  anything was included. */
  async #include(pick: ProjectPick, notice = true): Promise<boolean> {
    const result = await this.#api.projectsInclude(pick);
    if (!result.ok) {
      this.#host.fail(projectsErrorText(result.error));
      return false;
    }
    await this.reload();
    const { included, undo } = result.value;
    if (included === 0) return false;
    if (!notice) return true;
    this.#host.notice({
      text: tn("projects.included", included),
      actions:
        undo === null
          ? []
          : [{ label: t("fileOps.undo"), run: () => void this.#undo(undo, included) }],
      closeAfterMs: NOTICE_MS,
    });
    return true;
  }

  async #undo(token: number, count: number): Promise<void> {
    const result = await this.#api.projectsUndo(token);
    if (!result.ok) {
      this.#host.fail(projectsErrorText(result.error));
      this.#host.refocus();
      return;
    }
    await this.reload();
    // the Undo button went with its notice: focus to the strip (asking again) | the list; the reader hears what happened
    this.#host.refocus();
    if (result.value.undone) this.announce(tn("projects.undone", count));
    if (result.value.permarkKept) this.#note(t("projects.undoKept"));
  }

  #note(text: string): void {
    this.#host.notice({ text, actions: [], closeAfterMs: NOTICE_MS });
  }

  async #run(call: Promise<Result<null>>): Promise<boolean> {
    const result = await call;
    if (!result.ok) {
      this.#host.fail(projectsErrorText(result.error));
      return false;
    }
    await this.reload();
    return true;
  }
}

/** A view without the rows of `uids` (a removal waiting for its notice to go). */
function withoutRows(view: ProjectsView | null, uids: ReadonlySet<string>): ProjectsView | null {
  if (view === null || uids.size === 0) return view;
  return { ...view, rows: view.rows.filter((r) => r.uid === null || !uids.has(r.uid)) };
}

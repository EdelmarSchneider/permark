// One toast at a time. A burst of changed documents updates the same toast in place ("4 docs changed");
// plain messages (a missing section, a failed copy) use the same slot.

import { BurstTracker } from "../burst";
import type { ChangeKind } from "../feed";

export type ToastAction = { label: string; run: () => void };

export type ToastModel =
  | {
      id: number;
      type: "change";
      count: number;
      path: string;
      kind: ChangeKind;
      atMs: number;
    }
  | {
      id: number;
      type: "message";
      text: string;
      tone: "info" | "warn";
      /** A button on the toast (Undo): it runs once and the toast goes away. */
      action?: ToastAction;
    }
  /** An app update; the text and buttons follow the updater state (`stores/updates.svelte.ts`). */
  | { id: number; type: "update" };

const AUTO_HIDE_MS = 6000;
/** A toast with a button waits longer: there is something to decide. */
const ACTION_HIDE_MS = 12000;
/** The update toast waits longer: it asks a question. */
const UPDATE_HIDE_MS = 20000;
const LEAVE_MS = 180;

class ToastStore {
  current = $state.raw<ToastModel | null>(null);
  leaving = $state(false);

  private burst = new BurstTracker();
  private nextId = 1;
  /** How long the current toast stays; null keeps it until it is dismissed. */
  private hideMs: number | null = AUTO_HIDE_MS;
  private hideTimer: ReturnType<typeof setTimeout> | null = null;
  private leaveTimer: ReturnType<typeof setTimeout> | null = null;

  /** A document changed: show the toast, or update its counter if a burst is in progress. */
  noteChange(path: string, kind: ChangeKind, now: number): void {
    const state = this.burst.note(path, kind, now);
    this.show({
      id: this.current?.type === "change" ? this.current.id : this.nextId++,
      type: "change",
      count: state.count,
      path: state.latest.path,
      kind: state.latest.kind,
      atMs: now,
    });
  }

  message(text: string, tone: "info" | "warn" = "info", action?: ToastAction): void {
    this.show(
      { id: this.nextId++, type: "message", text, tone, action },
      action ? ACTION_HIDE_MS : AUTO_HIDE_MS,
    );
  }

  /** The button of the toast was pressed: the toast goes, then the action runs. */
  runAction(): void {
    const current = this.current;
    const action = current?.type === "message" ? current.action : undefined;
    this.dismiss();
    action?.run();
  }

  /** An update is available (or being installed, when `sticky`: the toast then stays to show the progress). */
  update(sticky = false): void {
    this.show({ id: this.nextId++, type: "update" }, sticky ? null : UPDATE_HIDE_MS);
  }

  dismiss(): void {
    this.clearTimers();
    this.burst.reset();
    if (!this.current) return;
    this.leaving = true;
    this.leaveTimer = setTimeout(() => {
      this.current = null;
      this.leaving = false;
    }, LEAVE_MS);
  }

  /** Hover: keep the toast while the pointer is over it. */
  pause(): void {
    if (this.hideTimer !== null) clearTimeout(this.hideTimer);
    this.hideTimer = null;
  }

  resume(): void {
    if (!this.current || this.hideMs === null) return;
    this.pause();
    this.hideTimer = setTimeout(() => this.dismiss(), this.hideMs);
  }

  private show(model: ToastModel, hideMs: number | null = AUTO_HIDE_MS): void {
    this.clearTimers();
    this.leaving = false;
    this.current = model;
    this.hideMs = hideMs;
    if (hideMs !== null) this.hideTimer = setTimeout(() => this.dismiss(), hideMs);
  }

  private clearTimers(): void {
    if (this.hideTimer !== null) clearTimeout(this.hideTimer);
    if (this.leaveTimer !== null) clearTimeout(this.leaveTimer);
    this.hideTimer = null;
    this.leaveTimer = null;
  }

  clear(): void {
    this.clearTimers();
    this.burst.reset();
    this.current = null;
    this.leaving = false;
  }
}

export const toast = new ToastStore();

// Backend interface. Implementations: `tauri.ts` (real IPC), `mock.ts` (dev:mock + tests). Only `api/` calls Tauri.

import type { AppInfo } from "./generated/AppInfo";

export type { AppInfo };

/** IPC commands: name -> args + result. Mirror of `src-tauri/src/commands.rs`; DTOs from `generated/` (ts-rs). */
export interface Commands {
  app_info: { args: Record<string, never>; result: AppInfo };
}
export type CommandName = keyof Commands;

export type ApiError =
  { kind: "command"; command: CommandName; message: string } | { kind: "window"; message: string };

export type Result<T> = { ok: true; value: T } | { ok: false; error: ApiError };

export interface Api {
  appInfo(): Promise<Result<AppInfo>>;
  /** Shows the main window (created hidden: first paint already themed). */
  showWindow(): Promise<Result<void>>;
}

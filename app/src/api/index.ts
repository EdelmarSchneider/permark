// Backend for this build: mock in `--mode mock`, Tauri otherwise (the unused one is tree-shaken).

import { mockApi } from "./mock";
import { tauriApi } from "./tauri";
import type { Api } from "./types";

export type * from "./types";
export type { Context } from "./generated/Context";
export type { WindowView } from "./generated/WindowView";
export type { WorkspaceView } from "./generated/WorkspaceView";
export type { StateRecovered } from "./generated/StateRecovered";
export type { LanguagePref } from "./generated/LanguagePref";
export type { ThemePref } from "./generated/ThemePref";
export type { Tone } from "./generated/Tone";

export const api: Api = import.meta.env.MODE === "mock" ? mockApi : tauriApi;

// Backend for this build: mock in `--mode mock`, Tauri otherwise (the unused one is tree-shaken).

import { mockApi } from "./mock";
import { tauriApi } from "./tauri";
import type { Api } from "./types";

export type { Api, ApiError, AppInfo, Result } from "./types";

export const api: Api = import.meta.env.MODE === "mock" ? mockApi : tauriApi;

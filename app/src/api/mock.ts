// Mock backend (`npm run dev:mock`, tests): same interface, no Tauri.

import type { Api } from "./types";

export const MOCK_VERSION = "0.0.0-mock";

export const mockApi: Api = {
  appInfo: () => Promise.resolve({ ok: true, value: { version: MOCK_VERSION } }),
  showWindow: () => Promise.resolve({ ok: true, value: undefined }),
};

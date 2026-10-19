import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { open } from "@tauri-apps/plugin-dialog";
import { asIpcError, errorMessage } from "./error";
import type { Api, CommandName, Commands, DropEvent, Result } from "./types";

/** Typed `invoke`: command name fixes args + result type; rejection -> `Result` error (backend shape when it is one). */
export async function call<C extends CommandName>(
  command: C,
  args: Commands[C]["args"],
): Promise<Result<Commands[C]["result"]>> {
  try {
    const value = await invoke<Commands[C]["result"]>(command, args);
    return { ok: true, value };
  } catch (error) {
    const backend = asIpcError(error);
    if (backend) return { ok: false, error: { kind: "backend", command, error: backend } };
    return { ok: false, error: { kind: "command", command, message: errorMessage(error) } };
  }
}

export const tauriApi: Api = {
  appInfo: () => call("app_info", {}),
  windowBoot: () => call("window_boot", {}),
  openPath: (path, placement) => call("open_path", { path, placement }),
  showWelcome: () => call("show_welcome", {}),
  listWorkspaces: () => call("list_workspaces", {}),
  focusWorkspace: (id) => call("focus_workspace", { id }),
  recentList: () => call("recent_list", {}),
  recentRemove: (path) => call("recent_remove", { path }),
  settingsGet: () => call("settings_get", {}),
  settingsSet: (change) => call("settings_set", { change }),
  projectSessionGet: (key) => call("project_session_get", { key }),
  projectSessionSet: (key, value) => call("project_session_set", { key, value }),
  on: (event, handler) => listen<Parameters<typeof handler>[0]>(event, (e) => handler(e.payload)),
  onDrop: (handler) =>
    getCurrentWebview().onDragDropEvent((event) => {
      const payload = event.payload;
      let mapped: DropEvent | null;
      switch (payload.type) {
        case "enter":
          mapped = { type: "over", count: payload.paths.length };
          break;
        case "over":
          mapped = null;
          break;
        case "leave":
          mapped = { type: "leave" };
          break;
        case "drop":
          mapped = { type: "drop", paths: payload.paths };
          break;
        default:
          mapped = payload satisfies never;
      }
      if (mapped) handler(mapped);
    }),
  pickFolder: async (title) => {
    const picked = await open({ directory: true, multiple: false, title });
    return typeof picked === "string" ? picked : null;
  },
  pickFile: async (title, filterName) => {
    const picked = await open({
      directory: false,
      multiple: false,
      title,
      filters: [{ name: filterName, extensions: ["md", "markdown", "mdown", "mdx", "txt"] }],
    });
    return typeof picked === "string" ? picked : null;
  },
  setTitle: async (title) => {
    try {
      await getCurrentWindow().setTitle(title);
    } catch {
      // the title only helps tell windows apart; nothing depends on it
    }
  },
  showWindow: async () => {
    try {
      await getCurrentWindow().show();
      return { ok: true, value: undefined };
    } catch (error) {
      return { ok: false, error: { kind: "window", message: errorMessage(error) } };
    }
  },
};

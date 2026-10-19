import { convertFileSrc, invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { open } from "@tauri-apps/plugin-dialog";
import { asIpcError, errorMessage } from "./error";
import type {
  Api,
  CommandName,
  Commands,
  DropEvent,
  PluginEnvelope,
  PluginEventName,
  PluginMethod,
  PluginMethods,
  Result,
} from "./types";

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

/** `pm-asset` URL for a project path: `<base>/<workspace>/<path>` (the base differs per platform; Tauri knows it and
 *  encodes the whole path once, slashes included: the backend decodes it once). */
export function assetUrl(
  convert: (path: string, protocol: string) => string,
  workspace: number,
  path: string,
  version: string | null,
): string {
  const base = convert(`${workspace}/${path}`, "pm-asset");
  return version === null ? base : `${base}?v=${encodeURIComponent(version)}`;
}

/** A plugin method: the key is `<plugin>.<method>`; the backend takes the two parts. */
async function pluginCall<M extends PluginMethod>(
  method: M,
  args: PluginMethods[M]["args"],
): Promise<Result<PluginMethods[M]["result"]>> {
  const [plugin = "", name = ""] = method.split(".", 2);
  try {
    const value = await invoke<PluginMethods[M]["result"]>("plugin_call", {
      plugin,
      method: name,
      args,
    });
    return { ok: true, value };
  } catch (error) {
    const command = "plugin_call";
    const backend = asIpcError(error);
    if (backend) return { ok: false, error: { kind: "backend", command, error: backend } };
    return { ok: false, error: { kind: "command", command, message: errorMessage(error) } };
  }
}

export const tauriApi: Api = {
  appInfo: () => call("app_info", {}),
  windowBoot: () => call("window_boot", {}),
  openPath: (path, placement) => call("open_path", { path, placement }),
  openNotes: (placement) => call("open_notes", { placement }),
  openInProject: (path) => call("open_in_project", { path }),
  showWelcome: () => call("show_welcome", {}),
  listWorkspaces: () => call("list_workspaces", {}),
  focusWorkspace: (id) => call("focus_workspace", { id }),
  recentList: () => call("recent_list", {}),
  recentRemove: (path) => call("recent_remove", { path }),
  settingsGet: () => call("settings_get", {}),
  settingsSet: (change) => call("settings_set", { change }),
  projectSessionGet: (key) => call("project_session_get", { key }),
  projectSessionSet: (key, value) => call("project_session_set", { key, value }),
  treeOpen: (filter) => call("tree_open", { filter }),
  treeSetFilter: (filter) => call("tree_set_filter", { filter }),
  treeOpenFolder: (path) => call("tree_open_folder", { path }),
  treeKeep: (path) => call("tree_keep", { path }),
  treeClose: () => call("tree_close", {}),
  planMove: (moves, updateLinks) => call("fileops_plan_move", { moves, updateLinks }),
  applyMove: (planId, updateLinks) => call("fileops_apply_move", { planId, updateLinks }),
  deleteFiles: (paths) => call("fileops_delete", { paths }),
  newEntry: (dir, name, kind) => call("fileops_new", { dir, name, kind }),
  undoFileOp: () => call("fileops_undo", {}),
  absPath: (path) => call("file_abs_path", { path }),
  openExternal: (path, mode) => call("file_open_external", { path, mode }),
  showDocument: (path) => call("show_document", { path }),
  openDocument: (path, known) => call("document_open", { path, known }),
  closeDocument: () => call("document_close", {}),
  documentText: (path) => call("document_text", { path }),
  resolveLinks: (doc, hrefs) => call("link_resolve", { doc, hrefs }),
  resolveCodeRefs: (doc, written) => call("code_refs_resolve", { doc, written }),
  openInEditor: (path, line, col) => call("open_in_editor", { path, line, col }),
  openWeb: (url) => call("open_web", { url }),
  editSource: (path) => call("edit_source", { path }),
  editSegments: (text) => call("edit_segments", { text }),
  editSave: (path, seenHash, edits) => call("edit_save", { path, seenHash, edits }),
  editSaveAs: (path, text, exact) => call("edit_save_as", { path, text, exact }),
  editWriteAsset: async (doc, ext, bytes) => {
    try {
      const value = await invoke<string>("edit_write_asset", bytes, {
        headers: { "pm-doc": encodeURIComponent(doc), "pm-ext": encodeURIComponent(ext) },
      });
      return { ok: true, value };
    } catch (error) {
      const backend = asIpcError(error);
      const command = "edit_write_asset";
      if (backend) return { ok: false, error: { kind: "backend", command, error: backend } };
      return { ok: false, error: { kind: "command", command, message: errorMessage(error) } };
    }
  },
  editSetUnsaved: (path) => call("edit_set_unsaved", { path }),
  editHeadings: (path) => call("edit_headings", { path }),
  editLinkTargets: () => call("edit_link_targets", {}),
  configSheet: (path, text) => call("config_sheet", { path, text }),
  configEdit: (path, text, edit) => call("config_edit", { path, text, edit }),
  configSecrets: (path, text) => call("config_secrets", { path, text }),
  ignoreMatches: (path, text) => call("ignore_matches", { path, text }),
  analyze: (path, text, kind) => call("analyze", { path, text, kind }),
  searchStatus: () => call("search_status", {}),
  searchSections: (query, limit, options) => call("search_sections", { query, limit, options }),
  searchFiles: (query, limit, folderLimit, options) =>
    call("search_files", { query, limit, folderLimit, options }),
  searchRecent: (opened, limit, options) => call("search_recent", { opened, limit, options }),
  pluginCall,
  onPlugin: <E extends PluginEventName>(event: E, handler: (payload: PluginEnvelope<E>) => void) =>
    listen<PluginEnvelope<E>>(event, (e) => handler(e.payload)),
  windowClose: () => call("window_close", {}),
  assetUrl: (workspace, path, version) => assetUrl(convertFileSrc, workspace, path, version),
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
  pickFile: async (title, filterName, anyFile = false) => {
    const picked = await open({
      directory: false,
      multiple: false,
      title,
      filters: anyFile
        ? []
        : [{ name: filterName, extensions: ["md", "markdown", "mdown", "mdx", "txt"] }],
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

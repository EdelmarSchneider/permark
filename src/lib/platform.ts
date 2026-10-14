// Thin wrappers over the Tauri plugins the UI needs besides `api.ts`: dialogs, the opener for web links,
// drag and drop, the image protocol, the clipboard and the app updater. With `VITE_MOCK=1` (a plain browser) they degrade to harmless stand-ins.

import { convertFileSrc } from "@tauri-apps/api/core";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { openUrl } from "@tauri-apps/plugin-opener";
import { relaunch } from "@tauri-apps/plugin-process";
import { check } from "@tauri-apps/plugin-updater";
import { t } from "./i18n";
import type { DownloadProgress } from "./updates";

export const isMock = import.meta.env.VITE_MOCK === "1";

const MOCK_FOLDER = "C:/Users/demo/trailmark-docs";
const MOCK_FILE = "C:/Users/demo/Downloads/proposal.md";

export async function pickFolder(): Promise<string | null> {
  if (isMock) return MOCK_FOLDER;
  const picked = await openDialog({ directory: true, multiple: false });
  return typeof picked === "string" ? picked : null;
}

export async function pickFile(): Promise<string | null> {
  if (isMock) return MOCK_FILE;
  const picked = await openDialog({
    directory: false,
    multiple: false,
    filters: [
      {
        name: t("dialog.filter.docs"),
        extensions: ["md", "markdown", "mdx", "txt", "json", "xml"],
      },
    ],
  });
  return typeof picked === "string" ? picked : null;
}

export async function openExternalUrl(url: string): Promise<void> {
  if (isMock) {
    window.open(url, "_blank", "noopener,noreferrer");
    return;
  }
  await openUrl(url);
}

const MOCK_IMAGE =
  "data:image/svg+xml;utf8," +
  encodeURIComponent(
    '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 520 90"><rect width="520" height="90" rx="10" fill="#8b929a" fill-opacity=".18"/><text x="260" y="52" text-anchor="middle" font-family="sans-serif" font-size="15" fill="#656d76">mock image</text></svg>',
  );

/** The URI scheme of the backend that serves the images of the open folder (`ASSET_SCHEME` in Rust). */
const ASSET_SCHEME = "tmasset";

/** URL the webview can load for an image of the open folder, given its vault path. */
export function assetUrl(path: string): string {
  return isMock ? MOCK_IMAGE : convertFileSrc(path, ASSET_SCHEME);
}

export type DropHandlers = {
  over(count: number): void;
  leave(): void;
  drop(paths: string[]): void;
};

/** Listens for files and folders dropped on the window. Resolves to the unsubscribe function. */
export async function watchFileDrop(handlers: DropHandlers): Promise<() => void> {
  if (isMock) {
    const count = (e: DragEvent): number => e.dataTransfer?.items.length || 1;
    const onOver = (e: DragEvent): void => {
      e.preventDefault();
      handlers.over(count(e));
    };
    const onLeave = (e: DragEvent): void => {
      if (e.relatedTarget === null) handlers.leave();
    };
    const onDrop = (e: DragEvent): void => {
      e.preventDefault();
      handlers.drop(Array.from({ length: count(e) }, () => MOCK_FOLDER));
    };
    window.addEventListener("dragover", onOver);
    window.addEventListener("dragleave", onLeave);
    window.addEventListener("drop", onDrop);
    return () => {
      window.removeEventListener("dragover", onOver);
      window.removeEventListener("dragleave", onLeave);
      window.removeEventListener("drop", onDrop);
    };
  }
  return getCurrentWebview().onDragDropEvent((event) => {
    const payload = event.payload;
    switch (payload.type) {
      case "enter":
        handlers.over(payload.paths.length);
        break;
      case "over":
        break;
      case "leave":
        handlers.leave();
        break;
      case "drop":
        handlers.drop(payload.paths);
        break;
    }
  });
}

/** Copies text to the clipboard; returns false when the webview refuses. */
export async function copyText(text: string): Promise<boolean> {
  try {
    await navigator.clipboard.writeText(text);
    return true;
  } catch {
    // fall back to a hidden textarea (older webviews, or no clipboard permission)
    const area = document.createElement("textarea");
    area.value = text;
    area.setAttribute("readonly", "");
    area.style.position = "fixed";
    area.style.opacity = "0";
    document.body.appendChild(area);
    area.select();
    let ok = false;
    try {
      ok = document.execCommand("copy");
    } catch {
      ok = false;
    }
    area.remove();
    return ok;
  }
}

/** A newer version of the app, as announced by the update server. */
export type AvailableUpdate = {
  version: string;
  /** Downloads and installs it, then restarts the app (on Windows the installer does the restart). */
  install(onProgress: (progress: DownloadProgress) => void): Promise<void>;
  /** Releases the update when it is not going to be installed. */
  close(): Promise<void>;
};

const wait = (ms: number): Promise<void> => new Promise((resolve) => setTimeout(resolve, ms));

/** The stand-in update of the browser mock; `?update=none` makes the mock report "up to date". */
async function mockCheckForUpdate(): Promise<AvailableUpdate | null> {
  await wait(700);
  if (new URLSearchParams(location.search).get("update") === "none") return null;
  return {
    version: "0.2.0",
    install: async (onProgress) => {
      const total = 4_000_000;
      onProgress({ kind: "started", total });
      for (let sent = 0; sent < total; sent += 500_000) {
        await wait(250);
        onProgress({ kind: "chunk", bytes: 500_000 });
      }
      onProgress({ kind: "finished" });
      await wait(400);
      location.reload();
    },
    close: async () => {},
  };
}

/** Asks the update server for a newer version. Resolves to null when this one is the latest; rejects when the server can't be reached. */
export async function checkForUpdate(): Promise<AvailableUpdate | null> {
  if (isMock) return mockCheckForUpdate();
  const update = await check();
  if (!update) return null;
  return {
    version: update.version,
    install: async (onProgress) => {
      await update.downloadAndInstall((event) => {
        switch (event.event) {
          case "Started":
            onProgress({ kind: "started", total: event.data.contentLength ?? null });
            break;
          case "Progress":
            onProgress({ kind: "chunk", bytes: event.data.chunkLength });
            break;
          case "Finished":
            onProgress({ kind: "finished" });
            break;
        }
      });
      await relaunch();
    },
    close: () => update.close(),
  };
}

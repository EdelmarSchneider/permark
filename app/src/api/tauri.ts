import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { errorMessage } from "./error";
import type { Api, CommandName, Commands, Result } from "./types";

/** Typed `invoke`: command name fixes args + result type; rejection -> `Result` error. */
export async function call<C extends CommandName>(
  command: C,
  args: Commands[C]["args"],
): Promise<Result<Commands[C]["result"]>> {
  try {
    const value = await invoke<Commands[C]["result"]>(command, args);
    return { ok: true, value };
  } catch (error) {
    return { ok: false, error: { kind: "command", command, message: errorMessage(error) } };
  }
}

export const tauriApi: Api = {
  appInfo: () => call("app_info", {}),
  showWindow: async () => {
    try {
      await getCurrentWindow().show();
      return { ok: true, value: undefined };
    } catch (error) {
      return { ok: false, error: { kind: "window", message: errorMessage(error) } };
    }
  },
};

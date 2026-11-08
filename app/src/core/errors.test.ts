import { describe, expect, it } from "vitest";
import type { ApiError, IpcError } from "../api";
import { apiErrorText, ipcErrorText } from "./errors";

function unreadable(detail: string | null): IpcError {
  return { kind: "settingsUnreadable", subject: "C:/profile/settings.toml", detail };
}

describe("an unreadable settings file", () => {
  it("says why it can't be read and that nothing was changed", () => {
    expect(ipcErrorText(unreadable("notText"))).toBe(
      "PerMark can't read the settings file C:/profile/settings.toml (it isn't plain text). Nothing was changed.",
    );
    expect(ipcErrorText(unreadable("tooLarge"))).toBe(
      "PerMark can't read the settings file C:/profile/settings.toml (it's too large). Nothing was changed.",
    );
    const refused: ApiError = {
      kind: "backend",
      command: "settings_set",
      error: unreadable("locked"),
    };
    expect(apiErrorText(refused)).toBe(
      "PerMark can't read the settings file C:/profile/settings.toml (another program is using it). Nothing was changed.",
    );
  });

  it("still says what happened when the reason is not one it knows", () => {
    expect(ipcErrorText(unreadable(null))).toBe(
      "PerMark can't read the settings file C:/profile/settings.toml. Nothing was changed.",
    );
  });
});

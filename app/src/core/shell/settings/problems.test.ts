import { describe, expect, it } from "vitest";
import type { ApiError } from "../../../api";
import en from "../../i18n/locales/en";
import { problemKey, reasonCode } from "./problems";

describe("a refused setting in words", () => {
  it("words the groups file name that does not end in .toml", () => {
    const refused: ApiError = {
      kind: "backend",
      command: "settings_set",
      error: { kind: "invalidData", subject: "charactersFile", detail: "tomlOnly" },
    };
    expect(problemKey(reasonCode(refused))).toBe("settings.problem.tomlOnly");
    expect(en["settings.problem.tomlOnly"]).toBe("Use a file name ending in .toml.");
  });

  it("words a change refused because the shared settings file is not TOML", () => {
    const refused: ApiError = {
      kind: "backend",
      command: "project_config_set",
      error: { kind: "settingsUnreadable", subject: ".permark/config.toml", detail: "notToml" },
    };
    expect(problemKey(reasonCode(refused))).toBe("settings.problem.notToml");
  });

  it("falls back to a general sentence for a reason it does not know", () => {
    expect(problemKey("somethingNew")).toBe("settings.problem.other");
  });
});

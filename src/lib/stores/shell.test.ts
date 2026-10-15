import { beforeEach, describe, expect, it, vi } from "vitest";

// The backend is replaced: these tests are about what the store shows and tells the backend.
const fake = vi.hoisted(() => ({
  shellIntegrationState: vi.fn(),
  setShellContextMenu: vi.fn(),
  setShellFileAssociation: vi.fn(),
  refreshShellLabels: vi.fn(),
  openDefaultApps: vi.fn(),
}));
vi.mock("../api", () => ({ api: { ...fake, on: async () => () => {} } }));

import { language } from "../i18n";
import type { ShellState } from "../types";
import { rewordShellEntries, shell } from "./shell.svelte";
import { toast } from "./toast.svelte";

const off: ShellState = {
  supported: true,
  contextMenu: false,
  association: false,
  isDefault: false,
};

describe("the file manager entries", () => {
  beforeEach(() => {
    for (const fn of Object.values(fake)) fn.mockReset();
    shell.state = null;
    shell.busy = false;
    toast.clear();
    language.preference = "en";
  });

  it("shows what the system says, and nothing when it cannot be read", async () => {
    fake.shellIntegrationState.mockResolvedValue({ ...off, contextMenu: true });
    await shell.load();
    expect(shell.state).toEqual({ ...off, contextMenu: true });
    fake.shellIntegrationState.mockRejectedValue(new Error("no"));
    await shell.load();
    expect(shell.state).toBeNull();
  });

  it("writes the entry in the language of the interface and shows the state the backend answers", async () => {
    language.preference = "de";
    fake.setShellContextMenu.mockResolvedValue({ ...off, contextMenu: true });
    await shell.setContextMenu(true);
    expect(fake.setShellContextMenu).toHaveBeenCalledWith(true, "de");
    expect(shell.state).toEqual({ ...off, contextMenu: true });

    language.preference = "pt-BR";
    fake.setShellFileAssociation.mockResolvedValue({ ...off, association: true });
    await shell.setAssociation(true);
    expect(fake.setShellFileAssociation).toHaveBeenCalledWith(true, "pt-BR");
    expect(shell.state).toEqual({ ...off, association: true });
    expect(shell.busy).toBe(false);
  });

  it("tells the person when a change fails, and shows the registry as it is", async () => {
    fake.setShellContextMenu.mockRejectedValue({ kind: "shell", message: "x", detail: "denied" });
    fake.shellIntegrationState.mockResolvedValue({ ...off, contextMenu: true });
    await shell.setContextMenu(false);
    expect(toast.current).toMatchObject({ type: "message", tone: "warn" });
    expect(shell.state).toEqual({ ...off, contextMenu: true });
    expect(shell.busy).toBe(false);
  });

  it("opens the Windows page for the default app, and says so when it cannot", async () => {
    fake.openDefaultApps.mockResolvedValue(undefined);
    await shell.makeDefault();
    expect(fake.openDefaultApps).toHaveBeenCalledOnce();
    expect(toast.current).toBeNull();
    fake.openDefaultApps.mockRejectedValue({ kind: "internal", message: "x", detail: "no" });
    await shell.makeDefault();
    expect(toast.current).toMatchObject({ type: "message", tone: "warn" });
  });

  it("rewords the entries in the new language, quietly", async () => {
    language.preference = "es";
    fake.refreshShellLabels.mockResolvedValue(undefined);
    await rewordShellEntries();
    expect(fake.refreshShellLabels).toHaveBeenCalledWith("es");
    fake.refreshShellLabels.mockRejectedValue(new Error("no"));
    await rewordShellEntries();
    expect(toast.current).toBeNull();
  });
});

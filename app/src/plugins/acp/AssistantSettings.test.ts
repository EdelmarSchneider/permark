// @vitest-environment jsdom
// Settings > Assistant: the page the plugin adds (app group, only with a folder open, right after Local AI; gone when
// the plugin is off), the agent in use (its name | "Your command", the command that runs), Change… (the panel's
// picker over the page that closes), the own command with a live preview of the program that would start (or why
// not, in words), Save and Cancel, the keyboard after Save, the Snap line instead of the form.
import "../../core/testing/media";
import { flushSync, mount, tick, unmount } from "svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { MOCK_AGENTS } from "../../api/mockAcp";
import { t } from "../../core/i18n";
import {
  ContributionRegistry,
  PluginHost,
  pluginLoaders,
} from "../../core/plugins/registry.svelte";
import { ui } from "../../core/shell/ui.svelte";
import AssistantSettings from "./AssistantSettings.svelte";
import { ACP_PAGE, acp } from "./store.svelte";

let component: Record<string, unknown> | null = null;
let root: HTMLElement;

function open(): void {
  component = mount(AssistantSettings, { target: root, props: {} });
  flushSync();
}

function buttonNamed(name: string): HTMLButtonElement {
  const found = [...root.querySelectorAll("button")].find((b) => b.textContent?.trim() === name);
  if (!(found instanceof HTMLButtonElement)) throw new Error(`no button ${name}`);
  return found;
}

function field(): HTMLInputElement {
  const found = root.querySelector<HTMLInputElement>("form input");
  if (found === null) throw new Error("no command field");
  return found;
}

function type(text: string): void {
  field().value = text;
  field().dispatchEvent(new Event("input", { bubbles: true }));
  flushSync();
}

/** The line that says what the typed command would start (polite: it changes while the person types). */
function preview(): HTMLElement {
  const found = root.querySelector<HTMLElement>('form [aria-live="polite"]');
  if (found === null) throw new Error("no preview line");
  return found;
}

async function until(done: () => boolean): Promise<void> {
  await vi.waitFor(() => expect(done()).toBe(true), { timeout: 2000, interval: 10 });
  flushSync();
}

/** The person's own command in use; the list as the mock computer has it. */
function ownCommand(command: string): void {
  acp.status = {
    state: "idle",
    agent: command,
    agentName: null,
    chosen: true,
    error: null,
    methods: [],
    skippedFiles: 0,
    last: null,
    options: [],
  };
  acp.catalog = {
    status: "ready",
    agents: [
      ...MOCK_AGENTS.map((a) => ({ ...a, chosen: false })),
      {
        id: "custom",
        name: "",
        description: "",
        version: "",
        how: "custom",
        needs: null,
        command,
        website: null,
        chosen: true,
      },
    ],
  };
}

beforeEach(() => {
  vi.spyOn(acp, "loadAgents").mockResolvedValue();
  vi.spyOn(acp, "resolve").mockImplementation(async (command) =>
    command.startsWith("./")
      ? { program: null, error: "relativeProgram" }
      : command.startsWith("missing")
        ? { program: null, error: null }
        : { program: `C:\\tools\\${command.split(" ")[0] ?? ""}.exe`, error: null },
  );
  acp.clear();
  acp.status = {
    state: "idle",
    agent: "npx -y @agentclientprotocol/claude-agent-acp@0.108.0",
    agentName: "Claude Agent",
    chosen: true,
    error: null,
    methods: [],
    skippedFiles: 0,
    last: null,
    options: [],
  };
  acp.catalog = { status: "ready", agents: MOCK_AGENTS.map((a) => ({ ...a, chosen: false })) };
  root = document.createElement("div");
  document.body.append(root);
});

afterEach(() => {
  if (component) unmount(component);
  component = null;
  root.remove();
  acp.clear();
  ui.closeSettingsPage();
  vi.restoreAllMocks();
});

describe("what the plugin adds", () => {
  it("a page in the app's settings, only with a folder open, right after Local AI; nothing left when off", async () => {
    const registry = new ContributionRegistry();
    const host = new PluginHost(registry, pluginLoaders());
    await host.enable("acp");
    const page = registry.ownedBy("acp").find((c) => c.kind === "settingsPage");
    expect(page).toMatchObject({ id: ACP_PAGE, label: "acp.label", group: "app", order: 36 });
    expect(page?.requires).toEqual({
      needs: [{ need: "workspace", kinds: ["project"] }],
      prefers: [],
    });
    expect(host.disable("acp"), "contributions left after deactivate").toBe(0);
    expect(registry.items).toHaveLength(0);
  });
});

describe("the Assistant settings page", () => {
  it("names the agent in use and the command that runs", () => {
    open();
    expect(root.textContent).toContain(t("acp.page.inUse"));
    expect(root.textContent).toContain("Claude Agent");
    expect(root.textContent).toContain("npx -y @agentclientprotocol/claude-agent-acp@0.108.0");
    // what is sent and what it may cost, in the words the picker uses
    expect(root.textContent).toContain(t("acp.pick.intro"));
  });

  it("an own command in use is named as the person's, its command shown and kept in the field", () => {
    ownCommand('"C:\\a b\\node.exe" agent.mjs');
    open();
    expect(root.textContent).toContain(t("acp.ownCommand"));
    expect(field().value).toBe('"C:\\a b\\node.exe" agent.mjs');
  });

  it("none chosen yet: says so", () => {
    if (acp.status === null) throw new Error("no status");
    acp.status = { ...acp.status, chosen: false, agent: "", agentName: null };
    open();
    expect(root.textContent).toContain(t("acp.page.none"));
  });

  it("previews the program the typed command would start, or why not, as the person types", async () => {
    open();
    expect(preview().textContent?.trim()).toBe("");
    type("my-agent --acp");
    await until(() => preview().textContent?.includes("C:\\tools\\my-agent.exe") === true);
    expect(preview().textContent?.trim()).toBe(
      t("acp.preview.starts", { program: "C:\\tools\\my-agent.exe" }),
    );
    type("./agent");
    await until(() => preview().textContent?.trim() === t("acp.error.relativeProgram"));
    type("missing-agent");
    await until(() => preview().textContent?.trim() === t("acp.why.notFound"));
    // an empty field says nothing (and asks nothing)
    const asked = vi.mocked(acp.resolve).mock.calls.length;
    type("   ");
    await new Promise((resolve) => setTimeout(resolve, 400));
    flushSync();
    expect(preview().textContent?.trim()).toBe("");
    expect(vi.mocked(acp.resolve).mock.calls.length).toBe(asked);
    // a quick run of keys asks once, for the last text
    vi.mocked(acp.resolve).mockClear();
    type("a");
    type("ab");
    type("abc");
    await until(() => preview().textContent?.includes("abc.exe") === true);
    expect(vi.mocked(acp.resolve).mock.calls).toEqual([["abc"]]);
  });

  it("the field is named, says it starts a program, and points at its preview", () => {
    open();
    const input = field();
    const label = root.querySelector(`label[for="${input.id}"]`);
    expect(label?.textContent).toContain(t("acp.pick.command"));
    const described = (input.getAttribute("aria-describedby") ?? "").split(" ");
    const texts = described.map((id) => document.getElementById(id));
    expect(texts.some((el) => el?.textContent?.includes(t("acp.pick.commandWarning")))).toBe(true);
    expect(texts).toContain(preview());
  });

  it("Save keeps the typed command; the keyboard stays in the field and the change is said", async () => {
    const choose = vi.spyOn(acp, "choose").mockImplementation(async () => {
      ownCommand("my-agent --acp");
      return true;
    });
    open();
    type("my-agent --acp");
    field().focus();
    buttonNamed(t("acp.pick.useCommand")).click();
    await until(() => preview().textContent?.trim() === t("acp.page.saved"));
    expect(choose).toHaveBeenCalledWith({ command: "my-agent --acp" });
    expect(document.activeElement).toBe(field());
    expect(root.textContent).toContain(t("acp.ownCommand"));
  });

  it("a refused Save says why; Cancel puts back the command in use", async () => {
    vi.spyOn(acp, "choose").mockImplementation(async () => {
      acp.choiceRefusal = t("acp.error.openQuote");
      return false;
    });
    ownCommand("my-agent --acp");
    open();
    type('"my agent');
    buttonNamed(t("acp.pick.useCommand")).click();
    await until(() => root.querySelector('[role="alert"]') !== null);
    expect(root.querySelector('[role="alert"]')?.textContent).toContain(t("acp.error.openQuote"));
    buttonNamed(t("common.cancel")).click();
    flushSync();
    expect(field().value).toBe("my-agent --acp");
  });

  it("Cancel is there only while the text differs from the command in use; the field keeps the keyboard", () => {
    ownCommand("my-agent --acp");
    open();
    const cancel = buttonNamed(t("common.cancel"));
    expect(cancel.disabled).toBe(true);
    type("my-agent --acp --verbose");
    expect(cancel.disabled).toBe(false);
    cancel.focus();
    cancel.click();
    flushSync();
    expect(field().value).toBe("my-agent --acp");
    expect(cancel.disabled).toBe(true);
    expect(document.activeElement).toBe(field());
  });

  it("while the agent works Save waits and says why", () => {
    if (acp.status === null) throw new Error("no status");
    acp.status = { ...acp.status, state: "working" };
    open();
    type("my-agent");
    expect(buttonNamed(t("acp.pick.useCommand")).disabled).toBe(true);
    expect(root.textContent).toContain(t("acp.error.busy"));
  });

  it("Change… leaves the settings for the panel's picker", async () => {
    const openPicker = vi.spyOn(acp, "openPicker").mockImplementation(() => {});
    ui.openSettingsPage(ACP_PAGE);
    open();
    buttonNamed(t("acp.page.change")).click();
    await tick();
    await until(() => openPicker.mock.calls.length === 1);
    expect(ui.settingsPage).toEqual({ status: "closed" });
  });

  it("inside a Snap: one line says nothing can start, no form", () => {
    if (acp.status === null) throw new Error("no status");
    acp.status = { ...acp.status, needs: "snap" };
    open();
    expect(root.textContent).toContain(t("acp.snap"));
    expect(root.querySelector("form")).toBeNull();
  });
});

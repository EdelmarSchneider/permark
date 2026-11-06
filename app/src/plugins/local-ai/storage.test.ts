// @vitest-environment jsdom
// The Storage card against the mock backend: folder + what it holds, Show in folder, refusals in words, Change held
// while a download runs, the move | keep question (Escape, Cancel, focus back), a move with progress (done, cancelled,
// failed), Back to default, an unusable folder. One mock per file: the tests run in order on one store, each starting
// where the one before left it (noted per test).
import { flushSync, mount, unmount } from "svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("../../api", async () => {
  const { createMockApi } = await import("../../api/mock");
  return { api: createMockApi() };
});

const { api } = await import("../../api");
const { MOCK_FOLDER } = await import("../../api/mock");
const { MOCK_DEFAULT_STORE, MOCK_FREE_SPACE, MOCK_PROFILE } = await import("../../api/mockLocalAi");
const { sizeText } = await import("../../core/i18n");
const { localAi } = await import("./store.svelte");
const { default: LocalAiSection } = await import("./LocalAiSection.svelte");

const mock = api as unknown as ReturnType<typeof import("../../api/mock").createMockApi>;
const probe = mock.probe.localAi;

const SMALL = "fachtext-uebersetzer-q4_0";
const NEW_PLACE = "D:\\Models";
const OTHER_PLACE = "E:\\Other";

let target: HTMLElement;
let component: Record<string, unknown> | null = null;

async function until(check: () => boolean, what: string): Promise<void> {
  await vi.waitFor(() => expect(check(), what).toBe(true), { timeout: 10_000, interval: 10 });
}

function card(): HTMLElement {
  const found = Array.from(target.querySelectorAll<HTMLElement>("section")).find(
    (s) => s.querySelector("h3")?.textContent === "Storage",
  );
  if (!found) throw new Error("no Storage card");
  return found;
}

const text = (): string => (card().textContent ?? "").replace(/\s+/g, " ");

function button(action: string): HTMLButtonElement | null {
  return card().querySelector<HTMLButtonElement>(`[data-action="${action}"]`);
}

function press(action: string): void {
  const found = button(action);
  if (!found) throw new Error(`no ${action}: ${text()}`);
  found.click();
  flushSync();
}

function alerts(): string[] {
  return Array.from(card().querySelectorAll('[role="alert"]')).map((a) => a.textContent ?? "");
}

const panel = (): HTMLElement | null => card().querySelector<HTMLElement>(".panel");

/** Change -> the picker answers `path`. */
async function changeTo(path: string): Promise<void> {
  vi.spyOn(api, "pickFolder").mockResolvedValueOnce(path);
  press("change");
}

/** Change -> the question about the installed models shows (focus on Move). */
async function askFor(path: string): Promise<void> {
  await changeTo(path);
  await until(() => panel() !== null, "the move | keep question");
  flushSync();
  await until(() => document.activeElement === button("move"), "focus on Move");
}

async function openPage(): Promise<void> {
  component = mount(LocalAiSection, { target });
  flushSync();
  await until(() => target.querySelector("li[data-model]") !== null, "the page shows");
  flushSync();
}

beforeEach(async () => {
  document.body.replaceChildren();
  target = document.createElement("div");
  document.body.append(target);
  probe.stepMs = 30;
  await openPage();
});

afterEach(async () => {
  if (component) await unmount(component);
  component = null;
  localAi.stop();
  probe.stopAll();
  vi.restoreAllMocks();
});

describe("the Storage card", () => {
  it("shows the folder (selectable) and what it holds", () => {
    const path = card().querySelector(".path");
    expect(path?.textContent).toBe(MOCK_DEFAULT_STORE);
    expect(text()).toContain("The default folder");
    const store = localAi.state?.store;
    expect(store).toBeDefined();
    expect(text()).toContain(`${sizeText(store?.usedBytes ?? 0)} used by 2 models`);
    expect(text()).toContain(`${sizeText(MOCK_FREE_SPACE)} free on this drive`);
    expect(text()).toContain(`${sizeText(store?.partialBytes ?? 0)} in unfinished downloads`);
    expect(button("default")).toBeNull();
    expect(alerts()).toEqual([]);
  });

  it("opens the folder in the system's file manager", async () => {
    press("reveal");
    await until(() => probe.calls.includes(`revealStore ${MOCK_DEFAULT_STORE}`), "revealed");
  });

  it("says in words why a folder is refused, and changes nothing", async () => {
    const picker = vi.spyOn(api, "pickFolder").mockResolvedValueOnce(`${MOCK_PROFILE}\\cache`);
    press("change");
    await until(() => alerts().length === 1, "refused");
    expect(picker).toHaveBeenCalledWith("Choose where to keep the models", MOCK_DEFAULT_STORE);
    expect(alerts()[0]).toBe(
      "This folder is inside PerMark's own data folder. Choose another folder.",
    );

    await changeTo(`${MOCK_DEFAULT_STORE}\\inner`);
    await until(() => alerts()[0]?.includes("inside a models folder") === true, "inside the store");

    await changeTo("relative\\models");
    await until(() => alerts()[0] === "Choose the full path of a folder.", "not absolute");

    await mock.openPath(MOCK_FOLDER, "thisTab");
    await changeTo(`${MOCK_FOLDER}/models`);
    await until(() => alerts()[0]?.includes("inside a project folder") === true, "in a project");

    expect(localAi.state?.store.path).toBe(MOCK_DEFAULT_STORE);
    expect(panel()).toBeNull();
  });

  it("holds Change while a model downloads, and says why", async () => {
    probe.stepMs = 10_000;
    await localAi.get(SMALL);
    flushSync();
    const change = button("change");
    expect(change?.disabled).toBe(true);
    const hint = card().querySelector(`#${change?.getAttribute("aria-describedby") ?? "none"}`);
    expect(hint?.textContent).toContain("You can change the folder when the download ends.");
    await localAi.cancel(SMALL);
    flushSync();
    expect(button("change")?.disabled).toBe(false);
  });

  it("asks whether the models move along; Escape and Cancel close the question, focus back on Change", async () => {
    await askFor(NEW_PLACE);
    expect(panel()?.textContent).toContain(
      `Move ${sizeText(localAi.state?.store.usedBytes ?? 0)} of models to the new place?`,
    );
    expect(button("keep")?.textContent).toBe("Keep them here and use the new folder");
    expect(button("change")?.getAttribute("aria-expanded")).toBe("true");
    panel()?.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
    flushSync();
    expect(panel()).toBeNull();
    await until(() => document.activeElement === button("change"), "focus back on Change");

    await askFor(NEW_PLACE);
    const cancel = Array.from(panel()?.querySelectorAll("button") ?? []).find(
      (b) => b.textContent === "Cancel",
    );
    cancel?.click();
    flushSync();
    expect(panel()).toBeNull();
    await until(() => document.activeElement === button("change"), "focus back on Change");
    expect(localAi.state?.store.path).toBe(MOCK_DEFAULT_STORE);
    expect(probe.calls.filter((c) => c.startsWith(`setStore ${NEW_PLACE}`))).toEqual([
      `setStore ${NEW_PLACE} -`,
      `setStore ${NEW_PLACE} -`,
    ]);
  });

  it("moves the models with progress, then uses the new folder", async () => {
    probe.stepMs = 100;
    await askFor(NEW_PLACE);
    press("move");
    await until(() => card().querySelector('[role="progressbar"]') !== null, "progress shows");
    expect(button("change")?.disabled).toBe(true);
    expect(text()).toContain("Moving the models");
    await until(() => document.activeElement === button("cancelMove"), "focus on Cancel");
    await until(() => localAi.state?.store.path === NEW_PLACE, "moved");
    flushSync();
    expect(probe.calls).toContain(`setStore ${NEW_PLACE} move`);
    expect(card().querySelector('[role="progressbar"]')).toBeNull();
    expect(text()).toContain("used by 2 models");
    expect(button("default")?.textContent).toBe("Back to default");
  });

  it("stops a move on Cancel (from D:\\Models)", async () => {
    probe.stepMs = 10_000;
    await askFor(OTHER_PLACE);
    press("move");
    await until(() => button("cancelMove") !== null, "moving");
    press("cancelMove");
    await until(() => localAi.state?.storeMove === null, "stopped");
    flushSync();
    expect(probe.calls).toContain("cancelMove");
    expect(localAi.state?.store.path).toBe(NEW_PLACE);
    await until(() => document.activeElement === button("change"), "focus back on Change");
  });

  it("says why a move failed (from D:\\Models)", async () => {
    probe.failMove = { kind: "disk", detail: "denied" };
    await askFor(OTHER_PLACE);
    press("move");
    await until(() => localAi.state?.storeMove?.kind === "failed", "the move fails");
    flushSync();
    expect(alerts()).toContain(
      "Couldn't move the models. The file couldn't be saved on this computer.",
    );
    expect(localAi.state?.store.path).toBe(NEW_PLACE);
  });

  it("keeps the models where they are and uses the new folder (from D:\\Models)", async () => {
    await askFor(OTHER_PLACE);
    press("keep");
    await until(() => localAi.state?.store.path === OTHER_PLACE, "the new folder in use");
    flushSync();
    expect(probe.calls).toContain(`setStore ${OTHER_PLACE} keep`);
    expect(text()).toContain("No models here yet");
    expect(alerts()).toEqual([]);
    await until(() => document.activeElement === button("change"), "focus back on Change");
  });

  it("goes back to the default folder (from E:\\Other, empty)", async () => {
    press("default");
    await until(() => localAi.state?.store.isDefault === true, "default again");
    flushSync();
    expect(card().querySelector(".path")?.textContent).toBe(MOCK_DEFAULT_STORE);
    expect(button("default")).toBeNull();
  });

  it("says why the folder in use cannot be reached, and offers the default", async () => {
    await changeTo("F:\\unplugged\\models");
    await until(() => localAi.state?.store.available === false, "the folder is gone");
    flushSync();
    expect(alerts()[0]).toContain("The models folder can't be found.");
    expect(button("reveal")?.disabled).toBe(true);

    const state = localAi.state;
    if (state === null) throw new Error("no state");
    localAi.state = {
      ...state,
      store: { ...state.store, unavailableReason: "insideProject" },
    };
    flushSync();
    expect(alerts()[0]).toContain("The models folder is inside a project folder");

    press("default");
    await until(() => localAi.state?.store.available === true, "back to the default");
    flushSync();
    expect(alerts()).toEqual([]);
  });
});

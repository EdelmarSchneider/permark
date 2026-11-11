// @vitest-environment jsdom
import { afterEach, describe, expect, it, vi } from "vitest";

vi.mock("../../api", async () => {
  const { createMockApi } = await import("../../api/mock");
  return { api: createMockApi() };
});
Object.defineProperty(window, "matchMedia", {
  value: () => ({ matches: false, addEventListener: () => {}, removeEventListener: () => {} }),
});

const { ContributionRegistry, CORE_OWNER } = await import("../plugins/registry.svelte");
const { registerCoreCommands } = await import("./core");
const { buildKeymap, conflicts } = await import("./keymap");
const { installKeys, isTypingTarget } = await import("./dispatch");
const { neutralContext } = await import("../conditions/context.svelte");
const { parseCombo } = await import("./combo");
const { en } = await import("../i18n/table");
const { documents } = await import("../doc/document.svelte");
const { editing } = await import("../editing/session.svelte");

describe("the core's commands", () => {
  const registry = new ContributionRegistry();
  const off = registerCoreCommands(registry);
  const commands = registry.of("command");
  const shortcuts = registry.of("shortcut");

  it("are owned by the core, have unique ids and a label in the language table", () => {
    expect(commands.length).toBeGreaterThan(20);
    expect(new Set(commands.map((c) => c.id)).size).toBe(commands.length);
    for (const command of commands) {
      expect(command.owner).toBe(CORE_OWNER);
      expect(command.id).toMatch(/^[a-z]+\.[A-Za-z]+[1-8]?$/);
      expect(Object.hasOwn(en, command.label), command.id).toBe(true);
    }
  });

  it("have default keys that parse, point at a command and do not fight", () => {
    const ids = new Set(commands.map((c) => c.id));
    for (const shortcut of shortcuts) {
      expect(parseCombo(shortcut.keys), shortcut.keys).not.toBeNull();
      expect(ids.has(shortcut.command), shortcut.command).toBe(true);
    }
    const map = buildKeymap(
      shortcuts.map((s) => ({ command: s.command, keys: s.keys })),
      {},
      [...ids],
    );
    const requiresOf = (id: string) => commands.find((c) => c.id === id)?.requires;
    expect(conflicts(map, requiresOf)).toEqual([]);
    // the same on macOS (Ctrl+Tab there is the Control key, its own combo)
    const mac = buildKeymap(
      shortcuts.map((s) => ({ command: s.command, keys: s.keys })),
      {},
      [...ids],
      true,
    );
    expect(conflicts(mac, requiresOf)).toEqual([]);
    expect(mac.combos.get("tab.next")).toEqual(["Ctrl+Tab", "Mod+Alt+ArrowRight"]);
    expect(map.combos.get("tab.next")).toEqual(["Mod+Tab", "Mod+Alt+ArrowRight"]);
  });

  it("keep the box commands out of the palette and name the keys the app is known by", () => {
    const hidden = commands.filter((c) => c.inPalette === false).map((c) => c.id);
    expect(hidden.sort()).toEqual([
      "doc.selectAll",
      "palette.open",
      "search.everywhere",
      "search.quickOpen",
      ...[1, 2, 3, 4, 5, 6, 7, 8].map((n) => `tab.select${n}`),
      "tab.selectLast",
    ]);
    const keyOf = (id: string): string | undefined => shortcuts.find((s) => s.command === id)?.keys;
    expect(keyOf("search.quickOpen")).toBe("Mod+p");
    expect(keyOf("search.all")).toBe("Mod+Shift+f");
    expect(keyOf("search.everywhere")).toBe("Mod+t");
    expect(keyOf("palette.open")).toBe("Mod+Shift+p");
  });

  it("move to the next and previous change with F7 and Shift+F7, and say when there is none", async () => {
    const keyOf = (id: string): string | undefined => shortcuts.find((s) => s.command === id)?.keys;
    expect(keyOf("doc.nextChange")).toBe("F7");
    expect(keyOf("doc.prevChange")).toBe("Shift+F7");
    const next = commands.find((c) => c.id === "doc.nextChange");
    expect(next?.when?.()).toBe(false); // no plugin gives a step
    await next?.run();
    const { windowStore } = await import("../window.svelte");
    expect(windowStore.notices.at(-1)).toMatchObject({ kind: "note", text: en["doc.noChange"] });
  });

  it("read the project folder again with F5, through the window's Explorer", async () => {
    const keyOf = (id: string): string | undefined => shortcuts.find((s) => s.command === id)?.keys;
    expect(keyOf("explorer.refresh")).toBe("F5");
    const refresh = commands.find((c) => c.id === "explorer.refresh");
    expect(refresh?.requires).toEqual({
      needs: [{ need: "workspace", kinds: ["project"] }],
      prefers: [],
    });
    expect(en[refresh?.label ?? "cmd.save"]).toBe("Refresh Explorer");
    const { setExplorer } = await import("../explorer/current");
    const asked = vi.fn(() => Promise.resolve());
    setExplorer({ refresh: asked } as unknown as Parameters<typeof setExplorer>[0]);
    await refresh?.run();
    expect(asked).toHaveBeenCalledOnce();
    // no tree in the window: nothing to ask
    setExplorer(null);
    await refresh?.run();
    expect(asked).toHaveBeenCalledOnce();
  });

  it("switch long lines wrapping with Alt+Z: saved, said by one note that closes, twice is back", async () => {
    const keyOf = (id: string): string | undefined => shortcuts.find((s) => s.command === id)?.keys;
    expect(keyOf("doc.wrapLines")).toBe("Alt+z");
    const { api } = await import("../../api");
    const { look } = await import("../look.svelte");
    const { windowStore } = await import("../window.svelte");
    const set = vi.spyOn(api, "settingsSet");
    const wrap = commands.find((c) => c.id === "doc.wrapLines");
    look.wrap = true;
    await wrap?.run();
    expect(look.wrap).toBe(false);
    expect(set).toHaveBeenLastCalledWith({ key: "wrapLines", value: false });
    expect(windowStore.notices.at(-1)).toMatchObject({
      kind: "timed",
      text: en["wrap.off"],
      closeAfterMs: 4000,
    });
    await wrap?.run();
    expect(look.wrap).toBe(true);
    expect(set).toHaveBeenLastCalledWith({ key: "wrapLines", value: true });
    expect(windowStore.notices.at(-1)).toMatchObject({ kind: "timed", text: en["wrap.on"] });
    // the new note replaced the one before: never a stack of notes that say the opposite
    const wrapNotes = (): number =>
      windowStore.notices.filter(
        (n) => n.kind === "timed" && (n.text === en["wrap.on"] || n.text === en["wrap.off"]),
      ).length;
    expect(wrapNotes()).toBe(1);
    for (let i = 0; i < 6; i++) await wrap?.run();
    expect(wrapNotes()).toBe(1);
    // refused: the views go back, the person is told
    set.mockResolvedValueOnce({
      ok: false,
      error: {
        kind: "backend",
        command: "settings_set",
        error: { kind: "io", subject: "settings.toml", detail: null },
      },
    });
    await wrap?.run();
    expect(look.wrap).toBe(true);
    const told = windowStore.notices.at(-1);
    expect(told?.kind === "timed" ? told.text : "").toMatch(/^Couldn't keep this setting/);
    expect(wrapNotes()).toBe(0);
    set.mockRestore();
  });

  describe("select all (Mod+A)", () => {
    const selectAll = commands.find((c) => c.id === "doc.selectAll");
    const selection = window.getSelection();
    const at = (id: string): HTMLElement => {
      const found = document.getElementById(id);
      if (!found) throw new Error(`no #${id}`);
      return found;
    };
    /** A window: bars, the Explorer, the page with a document, the right panel, and the places text is typed. */
    function drawWindow(): void {
      document.body.innerHTML =
        '<header id="top">Top bar <input id="field"><textarea id="area"></textarea></header>' +
        '<aside data-region="sidebar"><div role="tree" tabindex="0" id="tree">README.md</div></aside>' +
        '<main data-region="page"><div id="header">guide.md <button id="mode">Edit</button></div>' +
        '<div data-scroller tabindex="-1" id="pane"><h1>Title</h1>' +
        '<p>First <input type="checkbox" id="task"> words.</p></div></main>' +
        '<aside data-region="right"><button id="outline">Outline</button></aside>' +
        '<footer id="status">Status bar</footer>' +
        '<div contenteditable="true" tabindex="0" id="edit">x</div>' +
        '<div class="cm-content" tabindex="0" id="source">y</div>';
    }
    /** Exactly the contents of the document pane are selected. */
    function expectPaneSelected(): void {
      const pane = at("pane");
      expect(selection?.rangeCount).toBe(1);
      const range = selection?.getRangeAt(0);
      expect(range?.startContainer).toBe(pane);
      expect(range?.endContainer).toBe(pane);
      expect([range?.startOffset, range?.endOffset]).toEqual([0, pane.childNodes.length]);
      expect(selection?.toString()).toBe("TitleFirst  words.");
      for (const id of ["top", "tree", "header", "outline", "status"]) {
        expect(range?.intersectsNode(at(id)), id).toBe(false);
      }
    }
    /** No text is selected (a caret where the focus went is no selection). */
    function expectNothingSelected(): void {
      expect(selection?.isCollapsed).toBe(true);
      expect(selection?.toString()).toBe("");
    }

    it("is a command with its key, a label, and no entry in the palette", () => {
      const keyOf = (id: string): string | undefined =>
        shortcuts.find((s) => s.command === id)?.keys;
      expect(keyOf("doc.selectAll")).toBe("Mod+a");
      expect(selectAll?.inPalette).toBe(false);
      expect(en[selectAll?.label ?? "cmd.save"]).toBe("Select all");
      expect(selectAll?.requires).toEqual({
        needs: [{ need: "workspace", kinds: ["project", "singleFile", "notes"] }],
        prefers: [],
      });
    });

    let stopKeys: (() => void) | null = null;
    afterEach(async () => {
      stopKeys?.();
      stopKeys = null;
      editing.release();
      await documents.show(null);
      selection?.removeAllRanges();
      document.body.innerHTML = "";
    });

    it("the key press reaches it through the window's listener: from a ticked task box the document, in a field the field's own", async () => {
      drawWindow();
      await documents.show("README.md");
      const map = buildKeymap(
        shortcuts.map((s) => ({ command: s.command, keys: s.keys })),
        {},
        commands.map((c) => c.id),
      );
      stopKeys = installKeys(window, (event) => ({
        mac: false,
        keymap: map,
        commands,
        context: { ...neutralContext(), workspace: "project" },
        typing: isTypingTarget(event.target),
        overlay: "none",
        dialog: false,
      }));
      const press = (id: string): KeyboardEvent => {
        const event = new KeyboardEvent("keydown", {
          key: "a",
          code: "KeyA",
          ctrlKey: true,
          bubbles: true,
          cancelable: true,
        });
        at(id).focus();
        at(id).dispatchEvent(event);
        return event;
      };
      expect(press("task").defaultPrevented).toBe(true);
      expectPaneSelected();
      selection?.removeAllRanges();
      expect(press("field").defaultPrevented).toBe(false);
      expectNothingSelected();
    });

    it("in View selects the document and nothing of the window around it, wherever the keyboard is", async () => {
      drawWindow();
      await documents.show("README.md");
      for (const id of ["pane", "tree", "outline", "mode", "task"]) {
        selection?.removeAllRanges();
        at(id).focus();
        expect(document.activeElement, id).toBe(at(id));
        expect(selectAll?.when?.(), id).toBe(true);
        await selectAll?.run();
        expectPaneSelected();
      }
      // nothing has the keyboard (the body)
      selection?.removeAllRanges();
      at("task").blur();
      expect(selectAll?.when?.()).toBe(true);
      await selectAll?.run();
      expectPaneSelected();
    });

    it("leaves the key to the text where one has the keyboard: a field, a text area, the editors", () => {
      drawWindow();
      for (const id of ["field", "area", "edit", "source"]) {
        at(id).focus();
        expect(document.activeElement, id).toBe(at(id));
        expect(selectAll?.when?.(), id).toBe(false);
      }
    });

    it("selects nothing with no document shown, while editing, or with no page on screen", async () => {
      drawWindow();
      selection?.removeAllRanges();
      expect(documents.path).toBeNull();
      expect(selectAll?.when?.()).toBe(true);
      await selectAll?.run();
      expectNothingSelected();
      // editing, the keyboard outside the editor (a button of the header)
      await documents.show("README.md");
      await editing.enter("source", "README.md", "markdown", { kind: "explicit" });
      expect(editing.mode).toBe("source");
      at("mode").focus();
      expect(selectAll?.when?.()).toBe(true);
      await selectAll?.run();
      expectNothingSelected();
      // back in View the same press selects the document: it was the editor that held it back
      editing.release();
      await selectAll?.run();
      expect(selection?.toString()).toBe("TitleFirst  words.");
      // the page not drawn
      document.body.innerHTML = "<header>Top bar</header>";
      expect(documents.path).toBe("README.md");
      await selectAll?.run();
      expectNothingSelected();
    });
  });

  it("leave nothing behind when removed", () => {
    off();
    expect(registry.items).toEqual([]);
  });
});

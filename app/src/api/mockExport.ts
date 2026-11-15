// What the export plugin answers in the mock backend: the remembered page setup, a stand-in Save dialog that always
// picks the document's name in a fixed folder, and a print that starts at once. The real rendering, dialog and write
// are the shell's (tested there); the mock only feeds the window's flow.

import type { PageBox } from "./generated/PageBox";
import type { PageSetup } from "./generated/PageSetup";
import type { PageSize } from "./generated/PageSize";
import type { PrintCapabilities } from "./generated/PrintCapabilities";
import type { Saved } from "./generated/Saved";
import type { SetupView } from "./generated/SetupView";
import { MockPluginError } from "./mockOrganizing";
import type { PluginMethods } from "./types";

type ExportMethods = Pick<
  PluginMethods,
  | "export.setup.get"
  | "export.setup.set"
  | "export.capabilities"
  | "export.saveHtml"
  | "export.savePdf"
  | "export.print"
  | "export.cancel"
  | "export.open"
  | "export.reveal"
>;

export type ExportProbe = {
  /** The setup the person chose last (`null`: never chose one). */
  stored: PageSetup | null;
  capabilities: PrintCapabilities;
  /** The answer the next save gives instead of a written file (the person closed the Save dialog). */
  next: Saved | null;
  /** The refusal the next save | print gives (a read-only folder, the document itself chosen, no web view). */
  fail: MockPluginError | null;
  /** Calls made, by method name. */
  calls: string[];
  /** Files "written", path -> HTML. */
  saved: Map<string, string>;
};

/** The folder the stand-in Save dialog picks. */
export const MOCK_EXPORT_FOLDER = "C:/Users/me/Documents";

const DEFAULT_SETUP: PageSetup = {
  size: "a4",
  orientation: "portrait",
  margins: "normal",
  pageNumbers: true,
};

const PORTRAIT_MM: Record<PageSize, [number, number]> = {
  a4: [210, 297],
  a5: [148, 210],
  a3: [297, 420],
  letter: [215.9, 279.4],
  legal: [215.9, 355.6],
  tabloid: [279.4, 431.8],
};

const MARGIN_MM = { narrow: 10, normal: 20, wide: 30 } as const;

function pageOf(setup: PageSetup): PageBox {
  const [w, h] = PORTRAIT_MM[setup.size];
  const [widthMm, heightMm] = setup.orientation === "portrait" ? [w, h] : [h, w];
  return { widthMm, heightMm, marginMm: MARGIN_MM[setup.margins] };
}

function baseName(source: string): string {
  const name = source.split(/[\\/]/).at(-1) ?? source;
  return name.replace(/\.[^.]+$/, "");
}

export function createMockExport(): {
  handlers: {
    [M in keyof ExportMethods]: (args: ExportMethods[M]["args"]) => ExportMethods[M]["result"];
  };
  probe: ExportProbe;
} {
  const probe: ExportProbe = {
    stored: null,
    capabilities: { pdf: true, print: true, pageNumbers: true },
    next: null,
    fail: null,
    calls: [],
    saved: new Map(),
  };
  const view = (): SetupView => {
    const setup = probe.stored ?? DEFAULT_SETUP;
    return { setup, page: pageOf(setup), stored: probe.stored !== null };
  };
  const refuse = (): void => {
    const fail = probe.fail;
    probe.fail = null;
    if (fail !== null) throw fail;
  };
  const save = (html: string, source: string, ext: string): Saved => {
    refuse();
    const next = probe.next;
    probe.next = null;
    if (next !== null) return next;
    const path = `${MOCK_EXPORT_FOLDER}/${baseName(source)}.${ext}`;
    probe.saved.set(path, html);
    return { outcome: "saved", path };
  };
  return {
    probe,
    handlers: {
      "export.setup.get": () => {
        probe.calls.push("setup.get");
        return view();
      },
      "export.setup.set": ({ setup }) => {
        probe.calls.push("setup.set");
        probe.stored = setup;
        return view();
      },
      "export.capabilities": () => probe.capabilities,
      "export.saveHtml": ({ html, source }) => {
        probe.calls.push("saveHtml");
        return save(html, source, "html");
      },
      "export.savePdf": ({ html, source }) => {
        probe.calls.push("savePdf");
        return save(html, source, "pdf");
      },
      "export.print": () => {
        probe.calls.push("print");
        refuse();
        return null;
      },
      "export.cancel": () => {
        probe.calls.push("cancel");
        return null;
      },
      "export.open": () => {
        probe.calls.push("open");
        return null;
      },
      "export.reveal": () => {
        probe.calls.push("reveal");
        return null;
      },
    },
  };
}

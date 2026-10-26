// The review of code and plain text as a read-only code view (CodeMirror): the text with line numbers and the
// grammar's colors, the lines of every change tinted, and before them a block with a bar (what kind of change,
// Keep | Revert) and the old lines. The view only draws what the backend said; it changes nothing by itself.

import {
  Compartment,
  EditorState,
  StateEffect,
  StateField,
  type Extension,
  type Range,
} from "@codemirror/state";
import {
  Decoration,
  EditorView,
  lineNumbers,
  WidgetType,
  type DecorationSet,
} from "@codemirror/view";
import type { ReviewChange } from "../../api";
import { codeLook, languageFor } from "../../core/editing/source/cm";
import { t } from "../../core/i18n";
import { kindWord } from "./blocks";

/** Old lines drawn for one change at most (a rewrite of a whole file would be thousands of rows). */
export const OLD_LINES_SHOWN = 300;
/** The height the view reserves for an old line before it is measured (the code font size times 1.6). */
const LINE = 21.6;

/** What the view draws: the changes, the one the keys act on, whether Keep is offered. */
export type LinesData = {
  changes: readonly ReviewChange[];
  total: number;
  current: number;
  canKeep: boolean;
};

export type LinesHost = {
  keep(index: number): void;
  revert(index: number): void;
};

/** The text as the backend numbers its lines: every line break as `\n`. */
export function lineText(text: string): string {
  return text.replace(/\r\n?/g, "\n");
}

function element(tag: string, className: string, text?: string): HTMLElement {
  const node = document.createElement(tag);
  node.className = className;
  if (text !== undefined) node.textContent = text;
  return node;
}

/** A line of old text, as written. */
function oldLine(line: string): HTMLElement {
  const row = element("div", "rv-old-line");
  row.append(element("span", "rv-old-text", line));
  return row;
}

/** The bar of a change and the old lines, drawn above the lines that replaced them. */
class HunkWidget extends WidgetType {
  constructor(
    private readonly change: ReviewChange,
    private readonly data: LinesData,
    private readonly host: LinesHost,
  ) {
    super();
  }

  override eq(other: HunkWidget): boolean {
    return (
      other.change === this.change &&
      other.data.total === this.data.total &&
      other.data.canKeep === this.data.canKeep &&
      (other.data.current === other.change.index) === (this.data.current === this.change.index)
    );
  }

  override get estimatedHeight(): number {
    return 30 + Math.min(this.change.old.length, OLD_LINES_SHOWN) * LINE;
  }

  override toDOM(): HTMLElement {
    const { change, data } = this;
    const current = data.current === change.index;
    const root = element("div", "rv-hunk");
    root.setAttribute("data-rv-change", String(change.index));
    const bar = element("div", `rv-code-bar${current ? " is-current" : ""}`);
    bar.setAttribute("role", "group");
    bar.setAttribute(
      "aria-label",
      t("review.change", {
        i: change.index + 1,
        n: data.total,
        kind: kindWord(change.kind),
      }),
    );
    const kind = t(
      change.kind === "edited"
        ? "review.kind.modified"
        : change.kind === "added"
          ? "review.kind.added"
          : "review.kind.removed",
      { i: change.index + 1, n: data.total },
    );
    bar.append(element("span", "rv-kind", kind));
    if (change.whitespace) bar.append(element("span", "rv-ws", t("review.whitespace")));
    const actions = element("span", "rv-actions");
    const act = (label: string, hint: string, run: () => void): void => {
      const button = element("button", "", label);
      button.setAttribute("type", "button");
      button.setAttribute("title", hint);
      button.addEventListener("click", (event) => {
        event.preventDefault();
        run();
      });
      actions.append(button);
    };
    if (data.canKeep) {
      act(t("review.keepLabel"), t("review.keepHint"), () => this.host.keep(change.index));
    }
    act(t("review.revertLabel"), t("review.revertHint"), () => this.host.revert(change.index));
    bar.append(actions);
    root.append(bar);
    if (change.old.length > 0) {
      const old = element("div", "rv-old");
      old.setAttribute("aria-label", t("review.removedLabel"));
      for (const line of change.old.slice(0, OLD_LINES_SHOWN)) old.append(oldLine(line));
      if (change.old.length > OLD_LINES_SHOWN) {
        old.append(
          element(
            "div",
            "rv-old-more",
            t("review.moreOld", { n: change.old.length - OLD_LINES_SHOWN }),
          ),
        );
      }
      root.append(old);
    }
    return root;
  }

  override ignoreEvent(): boolean {
    return true;
  }
}

/** The decorations of a text with its changes. Pure given the state. */
export function decorate(state: EditorState, data: LinesData, host: LinesHost): DecorationSet {
  const doc = state.doc;
  const ranges: Range<Decoration>[] = [];
  for (const change of data.changes) {
    const start = Math.min(change.start, doc.lines);
    const current = data.current === change.index;
    const at = start < doc.lines ? doc.line(start + 1).from : doc.length;
    ranges.push(
      Decoration.widget({
        widget: new HunkWidget(change, data, host),
        block: true,
        side: start < doc.lines ? -1 : 1,
      }).range(at),
    );
    const mark = Decoration.line({
      class: `rv-line ${change.kind === "added" ? "rv-line-added" : "rv-line-edited"}${current ? " is-current" : ""}`,
    });
    for (let n = start; n < Math.min(start + change.len, doc.lines); n++) {
      ranges.push(mark.range(doc.line(n + 1).from));
    }
  }
  return Decoration.set(ranges, true);
}

const setData = StateEffect.define<LinesData>();

/** What the code view offers the component around it. */
export type LinesView = {
  /** Shows `text` with these changes (the text is replaced when it is other than what is shown). */
  show(text: string, data: LinesData): void;
  /** Brings the change into view. */
  reveal(change: ReviewChange | undefined): void;
  /** Long lines wrap (the setting) or scroll sideways. */
  setWrap(on: boolean): void;
  destroy(): void;
};

export function createLinesView(options: {
  parent: HTMLElement;
  path: string;
  host: LinesHost;
  /** Long lines wrap (the setting; default on). */
  wrap?: boolean;
}): LinesView {
  const language = new Compartment();
  const wrapping = new Compartment();
  let data: LinesData = { changes: [], total: 0, current: -1, canKeep: false };
  const marks = StateField.define<DecorationSet>({
    create: (state) => decorate(state, data, options.host),
    update(value, tr) {
      const next = tr.effects.find((effect) => effect.is(setData))?.value;
      if (next !== undefined) data = next;
      if (next !== undefined || tr.docChanged) {
        return decorate(tr.state, data, options.host);
      }
      return value;
    },
    provide: (field) => EditorView.decorations.from(field),
  });
  const loaded = languageFor(options.path, false);
  const extensions: Extension[] = [
    lineNumbers(),
    EditorState.readOnly.of(true),
    EditorView.editable.of(false),
    wrapping.of(options.wrap === false ? [] : EditorView.lineWrapping),
    language.of(loaded instanceof Promise ? [] : loaded),
    codeLook,
    marks,
  ];
  const view = new EditorView({
    parent: options.parent,
    state: EditorState.create({ doc: "", extensions }),
  });
  if (loaded instanceof Promise) {
    void loaded.then((support) => view.dispatch({ effects: language.reconfigure(support) }));
  }
  return {
    show: (text, next) => {
      const doc = lineText(text);
      const same = view.state.doc.toString() === doc;
      view.dispatch({
        changes: same ? undefined : { from: 0, to: view.state.doc.length, insert: doc },
        effects: setData.of(next),
      });
    },
    reveal: (change) => {
      if (change === undefined) return;
      const doc = view.state.doc;
      const at = change.start < doc.lines ? doc.line(change.start + 1).from : doc.length;
      view.dispatch({ effects: EditorView.scrollIntoView(at, { y: "center" }) });
    },
    setWrap: (on) =>
      view.dispatch({ effects: wrapping.reconfigure(on ? EditorView.lineWrapping : []) }),
    destroy: () => view.destroy(),
  };
}

// Secret values in Source: a mask over each one, from the backend (the structure of the format knows which values
// are secret). Clicking a mask shows that one value until the editor closes. The ranges come with the text, and
// again a moment after typing stops; between those moments they move with the edits.

import { RangeSetBuilder, StateEffect, StateField, type Extension } from "@codemirror/state";
import {
  Decoration,
  EditorView,
  ViewPlugin,
  WidgetType,
  type DecorationSet,
  type ViewUpdate,
} from "@codemirror/view";
import { MASK } from "../../config/secrets";

export type SecretRange = { from: number; to: number };

/** Asks for the secret ranges of a text (UTF-16 offsets). */
export type LoadSecrets = (text: string) => Promise<readonly SecretRange[]>;

const PAUSE_MS = 300;
/** A text whose first answer never comes is shown anyway after this long. */
const SHOW_ANYWAY_MS = 1500;

const setRanges = StateEffect.define<readonly SecretRange[]>();
const reveal = StateEffect.define<number>();
/** The first answer came (or never will): the text may be shown. */
const ready = StateEffect.define<null>();

type MaskState = {
  ranges: readonly SecretRange[];
  /** Starts of the values shown. */
  shown: readonly number[];
  decorations: DecorationSet;
  /** No answer yet: nothing is shown, so a secret is never on screen before it can be masked. */
  pending: boolean;
};

class MaskWidget extends WidgetType {
  constructor(private readonly label: string) {
    super();
  }

  override eq(other: MaskWidget): boolean {
    return other.label === this.label;
  }

  override toDOM(view: EditorView): HTMLElement {
    const button = document.createElement("button");
    button.type = "button";
    button.className = "cm-secret";
    button.textContent = MASK;
    button.title = this.label;
    button.setAttribute("aria-label", this.label);
    button.addEventListener("click", () => {
      const at = view.posAtDOM(button);
      view.dispatch({ effects: reveal.of(at) });
    });
    return button;
  }

  override ignoreEvent(): boolean {
    return false;
  }
}

function build(
  ranges: readonly SecretRange[],
  shown: readonly number[],
  label: string,
): DecorationSet {
  const builder = new RangeSetBuilder<Decoration>();
  let last = 0;
  for (const range of ranges) {
    if (range.from < last || range.to <= range.from || shown.includes(range.from)) continue;
    builder.add(range.from, range.to, Decoration.replace({ widget: new MaskWidget(label) }));
    last = range.to;
  }
  return builder.finish();
}

/** The secret values of the document as masks. `label`: what a mask says to screen readers (click to show). */
export function secretMask(load: LoadSecrets, label: string): Extension {
  const field = StateField.define<MaskState>({
    create: () => ({ ranges: [], shown: [], decorations: Decoration.none, pending: true }),
    update(value, tr) {
      let { ranges, shown, pending } = value;
      let changed = false;
      if (tr.docChanged) {
        ranges = ranges
          .map((r) => ({ from: tr.changes.mapPos(r.from, 1), to: tr.changes.mapPos(r.to, -1) }))
          .filter((r) => r.to > r.from);
        shown = shown.map((p) => tr.changes.mapPos(p, 1));
        changed = true;
      }
      for (const effect of tr.effects) {
        if (effect.is(setRanges)) {
          ranges = effect.value;
          changed = true;
        } else if (effect.is(reveal)) {
          shown = [...shown, effect.value];
          changed = true;
        } else if (effect.is(ready) && pending) {
          pending = false;
          changed = true;
        }
      }
      return changed ? { ranges, shown, pending, decorations: build(ranges, shown, label) } : value;
    },
    provide: (f) => [
      EditorView.decorations.from(f, (state) => state.decorations),
      EditorView.atomicRanges.of((view) => view.state.field(f).decorations),
      EditorView.editorAttributes.from(f, (state) => ({
        class: state.pending ? "cm-pending" : "",
      })),
    ],
  });

  const fetcher = ViewPlugin.fromClass(
    class {
      #timer: ReturnType<typeof setTimeout> | null = null;
      #fallback: ReturnType<typeof setTimeout> | null;
      #seq = 0;
      #gone = false;

      constructor(private readonly view: EditorView) {
        this.#fallback = setTimeout(() => this.#ready(), SHOW_ANYWAY_MS);
        this.#schedule(0);
      }

      update(update: ViewUpdate): void {
        if (update.docChanged) this.#schedule(PAUSE_MS);
      }

      destroy(): void {
        this.#gone = true;
        if (this.#timer !== null) clearTimeout(this.#timer);
        if (this.#fallback !== null) clearTimeout(this.#fallback);
        this.#seq += 1;
      }

      #ready(): void {
        if (this.#fallback !== null) clearTimeout(this.#fallback);
        this.#fallback = null;
        if (!this.#gone) this.view.dispatch({ effects: ready.of(null) });
      }

      #schedule(delay: number): void {
        if (this.#timer !== null) clearTimeout(this.#timer);
        const seq = ++this.#seq;
        this.#timer = setTimeout(() => {
          this.#timer = null;
          const doc = this.view.state.doc;
          void load(doc.toString()).then(
            (ranges) => {
              // an answer for an older text is dropped: the edit that made it old asked again
              if (this.#gone || seq !== this.#seq || this.view.state.doc !== doc) return;
              this.view.dispatch({ effects: [setRanges.of(ranges), ready.of(null)] });
              if (this.#fallback !== null) clearTimeout(this.#fallback);
              this.#fallback = null;
            },
            () => this.#ready(),
          );
        }, delay);
      }
    },
  );

  const theme = EditorView.baseTheme({
    "&.cm-pending .cm-content": { visibility: "hidden" },
    ".cm-secret": {
      margin: "0",
      padding: "0 5px",
      border: "1px solid var(--border)",
      borderRadius: "4px",
      background: "var(--surface-2)",
      color: "var(--muted)",
      font: "inherit",
      letterSpacing: "0.04em",
      cursor: "pointer",
    },
    ".cm-secret:hover, .cm-secret:focus-visible": {
      color: "var(--text)",
      borderColor: "var(--border-strong)",
    },
  });

  return [field, fetcher, theme];
}

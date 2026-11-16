// The parts of `@plantuml/core` the diagram engines use (the package ships no types). `viz-global.js` is the Graphviz
// build (Viz.js) PlantUML lays out with; the Graphviz plugin draws with it too.

declare module "@plantuml/core" {
  export function renderToString(
    lines: string[],
    onSuccess: (svg: string) => void,
    onError: (message: string) => void,
    options?: { dark?: boolean },
  ): void;
}

declare module "@plantuml/core/viz-global.js" {
  /** A line Graphviz wrote while reading | laying out a graph. */
  export type VizMessage = { level?: "error" | "warning"; message: string };
  export type VizResult =
    | { status: "success"; output: string; errors: VizMessage[] }
    | { status: "failure"; output: undefined; errors: VizMessage[] };
  export interface Viz {
    render(source: string, options?: { format?: string; engine?: string }): VizResult;
  }
  export function instance(): Promise<Viz>;
}

/** Set by `viz-global.js` when it loads as a plain script (no module system to hand its exports to). */
declare var Viz: { instance(): Promise<import("@plantuml/core/viz-global.js").Viz> } | undefined;

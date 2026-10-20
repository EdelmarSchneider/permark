// The parts of `@plantuml/core` the engine uses (the package ships no types).

declare module "@plantuml/core" {
  export function renderToString(
    lines: string[],
    onSuccess: (svg: string) => void,
    onError: (message: string) => void,
    options?: { dark?: boolean },
  ): void;
}

declare module "@plantuml/core/viz-global.js" {
  export function instance(): Promise<unknown>;
}

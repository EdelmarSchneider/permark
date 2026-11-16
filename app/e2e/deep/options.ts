// Command line of `npm run deep`.

export const LAYERS = ["render", "security"] as const;
export type Layer = (typeof LAYERS)[number];

export type Options = {
  /** Layers to run (default: all). */
  only: readonly Layer[];
  /** Text goldens rewritten, differing | new pixel shots left in `pending/` for review. */
  update: boolean;
  /** Promote pending pixel shots matching this glob (`all`: every one) and stop. */
  accept: string | null;
  /** Light theme and View only, fewer sanitizer cases. */
  quick: boolean;
  /** Playwright's own filter (`--grep`), e.g. one page. */
  grep: string | null;
};

export const USAGE = `usage: npm run deep -- [--only render|security] [--update] [--accept <glob|all>] [--quick] [--grep <text>]
  --only      run one layer (render: corpus render goldens; security: sanitizer properties + attack pages)
  --update    rewrite text goldens; differing or new pixel shots go to <baselines>/pending/ (review page there)
  --accept    promote pending shots (glob over <page>/<shot>, * within a part, ** across) and record the machine
  --quick     light theme, View only, fewer sanitizer cases
  --grep      only the browser tests whose title matches`;

export class UsageError extends Error {}

export function parseOptions(args: readonly string[]): Options {
  const options: Options = { only: LAYERS, update: false, accept: null, quick: false, grep: null };
  for (let at = 0; at < args.length; at++) {
    const arg = args[at];
    const value = (): string => {
      const next = args[++at];
      if (next === undefined || next.startsWith("--")) throw new UsageError(`${arg} needs a value`);
      return next;
    };
    switch (arg) {
      case "--only": {
        const layer = value();
        const known = LAYERS.find((l) => l === layer);
        if (known === undefined) throw new UsageError(`unknown layer: ${layer}`);
        options.only = [known];
        break;
      }
      case "--update":
        options.update = true;
        break;
      case "--accept":
        options.accept = value();
        break;
      case "--quick":
        options.quick = true;
        break;
      case "--grep":
        options.grep = value();
        break;
      default:
        throw new UsageError(`unknown option: ${arg ?? ""}`);
    }
  }
  if (options.accept !== null && options.update) {
    throw new UsageError("--accept and --update are separate steps");
  }
  return options;
}

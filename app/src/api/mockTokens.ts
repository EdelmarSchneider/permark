// What the token count plugin answers in the mock backend: the vocabularies (a download that takes a moment and can be
// made to fail), and counts by a simple stand-in per tokenizer. In memory; the same shapes as the real plugin.

import type { CountResult } from "./generated/CountResult";
import type { TokenCountState } from "./generated/TokenCountState";
import type { VocabFailure } from "./generated/VocabFailure";
import type { VocabState } from "./generated/VocabState";
import { MockPluginError } from "./mockOrganizing";
import type { AppPluginEventName, AppPluginEvents, PluginMethods } from "./types";

type TokenMethods = Pick<
  PluginMethods,
  | "token-count.state"
  | "token-count.download"
  | "token-count.cancel"
  | "token-count.removeVocab"
  | "token-count.setOutline"
  | "token-count.count"
>;

type Encoding = "o200k" | "cl100k";
const SIZES: Record<Encoding, number> = { o200k: 3_613_922, cl100k: 1_681_126 };

export type TokensProbe = {
  /** Puts a vocabulary in a state (as the backend would announce it). */
  setVocab(encoding: Encoding, state: VocabState): void;
  /** The next download ends in this failure (`null`: it works). */
  failNext: VocabFailure | null;
  /** How long a download takes, in steps of 150 ms. */
  steps: number;
  /** Calls made, `method tokenizer`. */
  calls: string[];
  /** Texts the counter was asked about. */
  counted: string[];
};

/** A stand-in for each tokenizer: the real counts come from the backend. */
export function standIn(tokenizer: string, text: string): number {
  const per: Record<string, number> = {
    o200k: 4.1,
    cl100k: 3.9,
    claude: 3.5,
    gemini: 4.2,
    chars4: 4,
  };
  return Math.ceil([...text].length / (per[tokenizer] ?? 4));
}

function isEncoding(id: string): id is Encoding {
  return id === "o200k" || id === "cl100k";
}

export function createMockTokens(
  emit: <E extends AppPluginEventName>(event: E, payload: AppPluginEvents[E]) => void,
) {
  const vocab: Record<Encoding, VocabState> = {
    o200k: { kind: "absent" },
    cl100k: { kind: "absent" },
  };
  let outline = false;
  const timers = new Map<Encoding, ReturnType<typeof setInterval>>();
  const probe: TokensProbe = {
    setVocab: (encoding, state) => {
      stop(encoding);
      vocab[encoding] = state;
      announce();
    },
    failNext: null,
    steps: 4,
    calls: [],
    counted: [],
  };

  function state(): TokenCountState {
    return {
      vocab: {
        o200k: { state: vocab.o200k, bytes: SIZES.o200k },
        cl100k: { state: vocab.cl100k, bytes: SIZES.cl100k },
      },
      outline,
    };
  }

  function announce(): void {
    emit("token-count:state", state());
  }

  function encodingOf(tokenizer: string): Encoding {
    if (!isEncoding(tokenizer)) throw new MockPluginError("invalidData", tokenizer);
    return tokenizer;
  }

  function stop(encoding: Encoding): void {
    const timer = timers.get(encoding);
    if (timer !== undefined) clearInterval(timer);
    timers.delete(encoding);
  }

  const handlers: {
    [M in keyof TokenMethods]: (args: TokenMethods[M]["args"]) => TokenMethods[M]["result"];
  } = {
    "token-count.state": () => state(),
    "token-count.download": ({ tokenizer }) => {
      probe.calls.push(`download ${tokenizer}`);
      const encoding = encodingOf(tokenizer);
      if (vocab[encoding].kind !== "absent" && vocab[encoding].kind !== "failed") return state();
      const total = SIZES[encoding];
      vocab[encoding] = { kind: "downloading", done: 0, total };
      let step = 0;
      timers.set(
        encoding,
        setInterval(() => {
          step += 1;
          if (step < probe.steps) {
            vocab[encoding] = {
              kind: "downloading",
              done: Math.floor((total * step) / probe.steps),
              total,
            };
          } else {
            stop(encoding);
            const failure = probe.failNext;
            probe.failNext = null;
            vocab[encoding] =
              failure === null ? { kind: "ready" } : { kind: "failed", reason: failure };
          }
          announce();
        }, 150),
      );
      return state();
    },
    "token-count.cancel": ({ tokenizer }) => {
      probe.calls.push(`cancel ${tokenizer}`);
      const encoding = encodingOf(tokenizer);
      if (vocab[encoding].kind === "downloading") {
        stop(encoding);
        vocab[encoding] = { kind: "absent" };
        announce();
      }
      return state();
    },
    "token-count.removeVocab": ({ tokenizer }) => {
      probe.calls.push(`removeVocab ${tokenizer}`);
      const encoding = encodingOf(tokenizer);
      if (vocab[encoding].kind !== "downloading") {
        vocab[encoding] = { kind: "absent" };
        announce();
      }
      return state();
    },
    "token-count.setOutline": ({ on }) => {
      outline = on;
      announce();
      return state();
    },
    "token-count.count": ({ tokenizers, text, parts }) => {
      probe.counted.push(text);
      const lines = text.split("\n");
      return tokenizers.map((tokenizer): CountResult => {
        if (isEncoding(tokenizer) && vocab[tokenizer].kind !== "ready")
          return { status: "unavailable", tokenizer, reason: "noVocab" };
        const of = (from: number, to: number): string => {
          const first = Math.max(1, from);
          const slice = lines.slice(first - 1, Math.max(first - 1, to));
          return slice.map((l, at) => (first + at < lines.length ? `${l}\n` : l)).join("");
        };
        return {
          status: "ok",
          tokenizer,
          whole: standIn(tokenizer, text),
          parts: (parts ?? []).map(([from, to]) => standIn(tokenizer, of(from, to))),
        };
      });
    },
  };

  return { handlers, probe };
}

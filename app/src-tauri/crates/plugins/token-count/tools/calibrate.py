"""Fits the weights of the estimators (`src/estimate.rs`, `src/plugins/token-count/estimate.ts`) on the corpus.

    python -m pip install tiktoken numpy scipy sentencepiece
    python tools/calibrate.py <folder with the two .tiktoken files> [<Gemini tokenizer.model>]

Run it from the plugin crate folder. It prints the weights (milli-tokens per unit) and the error table. Measured
targets: `o200k` and `cl100k` (the rank files), `gemini` (the SentencePiece model Google's SDK uses to count Gemini
tokens locally, when a model file is given). Nothing is measured for `claude`: see `estimate.rs`. `generic` (the
estimator of `permark_core::tokens`) is fitted against `o200k` on the corpus plus the answers in
`calibration/agent-answers.json` (tab tables, JSON and TOON of the same data), which weigh as much as half the corpus.
"""

import json
import os
import sys

import numpy as np
import tiktoken
import tiktoken_ext.openai_public as public
from scipy.optimize import nnls
from tiktoken.load import load_tiktoken_bpe

HERE = os.path.dirname(os.path.abspath(__file__))
CORPUS = os.path.join(HERE, "..", "corpus")
FEATURES = ["space", "tab", "newline", "lower", "upper", "digit", "latinExt", "cjk", "greekCyr", "letterOther", "emoji", "symbol",
            "punct", "punctJoin", "word", "excess"]

EXCESS_FROM = 6  # characters of a word beyond this many count as "excess"
CJK = [(0x2E80, 0x2FDF), (0x3040, 0x30FF), (0x3100, 0x312F), (0x3190, 0x31FF), (0x3400, 0x4DBF), (0x4E00, 0x9FFF),
       (0xAC00, 0xD7AF), (0xF900, 0xFAFF), (0x20000, 0x3FFFF)]
GREEK_CYRILLIC = [(0x370, 0x52F), (0x1F00, 0x1FFF)]
LETTER_OTHER = [(0x530, 0x1EFF), (0x2C00, 0x2DFF), (0xA000, 0xABFF), (0xFB1D, 0xFDFF), (0xFE70, 0xFEFF)]
EMOJI = [(0x1F000, 0x1FAFF), (0x2600, 0x27BF), (0xFE00, 0xFE0F), (0x200D, 0x200D), (0xE0000, 0xE007F), (0x2B00, 0x2BFF)]
SPACE = {0x20, 0x09, 0xA0, 0x1680, 0x202F, 0x205F, 0x3000} | set(range(0x2000, 0x200B))
NEWLINE = {0x0A, 0x0D, 0x0B, 0x0C, 0x85, 0x2028, 0x2029}


def in_ranges(cp, ranges):
    return any(a <= cp <= b for a, b in ranges)


def class_of(cp):
    if cp == 0x09:
        return "tab"
    if cp in SPACE:
        return "space"
    if cp in NEWLINE:
        return "newline"
    if 0x30 <= cp <= 0x39:
        return "digit"
    if 0x41 <= cp <= 0x5A:
        return "upper"
    if 0x61 <= cp <= 0x7A:
        return "lower"
    if cp <= 0x7F:
        return "punct"
    if cp in (0xD7, 0xF7) or 0x80 <= cp <= 0xBF:
        return "symbol"
    if 0xC0 <= cp <= 0x36F or 0x1E00 <= cp <= 0x1EFF:
        return "latinExt"
    if in_ranges(cp, CJK):
        return "cjk"
    if in_ranges(cp, GREEK_CYRILLIC):
        return "greekCyr"
    if in_ranges(cp, LETTER_OTHER):
        return "letterOther"
    if in_ranges(cp, EMOJI):
        return "emoji"
    return "symbol"


def features(text):
    counts = dict.fromkeys(FEATURES, 0)
    run = 0
    previous = None
    for ch in text:
        name = class_of(ord(ch))
        # ASCII punctuation right after ASCII punctuation (`":"`, `},{`, `();`) mostly merges into the same token
        if name == "punct" and previous == "punct":
            counts["punctJoin"] += 1
        else:
            counts[name] += 1
        previous = name
        if name in ("lower", "upper", "digit", "latinExt"):
            if run == 0:
                counts["word"] += 1
            run += 1
            if run > EXCESS_FROM:
                counts["excess"] += 1
        else:
            run = 0
    return [counts[name] for name in FEATURES]


def load_encodings(folder):
    out = {}
    original = public.load_tiktoken_bpe
    for key, name in (("o200k", "o200k_base"), ("cl100k", "cl100k_base")):
        path = os.path.join(folder, name + ".tiktoken")
        public.load_tiktoken_bpe = lambda url, expected_hash=None, path=path: load_tiktoken_bpe(path)
        spec = getattr(public, name)()
        enc = tiktoken.Encoding(spec["name"], pat_str=spec["pat_str"], mergeable_ranks=spec["mergeable_ranks"],
                                special_tokens=spec["special_tokens"])
        out[key] = lambda text, enc=enc: len(enc.encode_ordinary(text))
    public.load_tiktoken_bpe = original
    return out


def chunks_of(text, target):
    pieces, current = [], ""
    for line in text.splitlines(keepends=True):
        current += line
        if len(current) >= target:
            pieces.append(current)
            current = ""
    if current:
        pieces.append(current)
    return pieces


def fit_generic(count, corpus_rows):
    """The weights of `permark_core::tokens::GENERIC`: o200k on the corpus rows + the agent answers (half weight)."""
    with open(os.path.join(HERE, "..", "calibration", "agent-answers.json"), encoding="utf-8") as f:
        answers = list(json.load(f).values())
    rows = [(r[1], r[2], 1.0 / len(corpus_rows)) for r in corpus_rows]
    rows += [(t, features(t), 0.5 / len(answers)) for t in answers]
    y = np.array([count(r[0]) for r in rows], dtype=float)
    ok = y > 0
    s = np.sqrt(np.array([r[2] for r in rows]))[ok]
    X = np.array([r[1] for r in rows], dtype=float)[ok]
    w, _ = nnls(X / y[ok][:, None] * s[:, None], s)
    rel = np.abs(X @ w - y[ok]) / y[ok]
    n = len(corpus_rows)
    corpus_ok = ok[:n].sum()
    return {
        "weights_milli": {name: round(float(v) * 1000) for name, v in zip(FEATURES, w)},
        "mean_abs_corpus_rows": float(np.mean(rel[:corpus_ok])),
        "mean_abs_answers": float(np.mean(rel[corpus_ok:])),
    }


def main():
    counters = load_encodings(sys.argv[1])
    if len(sys.argv) > 2:
        import sentencepiece

        sp = sentencepiece.SentencePieceProcessor(model_file=sys.argv[2])
        counters["gemini"] = lambda text: len(sp.encode(text))
    files = sorted(f for f in os.listdir(CORPUS) if f not in ("expected.json", "cases.json", "README.md"))
    rows = []  # (file, text, features)
    for name in files:
        with open(os.path.join(CORPUS, name), encoding="utf-8", newline="") as f:
            text = f.read()
        for target in (350, 900, 2500):
            for chunk in chunks_of(text, target):
                rows.append((name, chunk, features(chunk)))
        rows.append((name, text, features(text)))
    X = np.array([r[2] for r in rows], dtype=float)
    result = {"generic": fit_generic(counters["o200k"], rows)}
    for key, count in counters.items():
        y = np.array([count(r[1]) for r in rows], dtype=float)
        ok = y > 0
        A = X[ok] / y[ok][:, None]
        w, _ = nnls(A, np.ones(A.shape[0]))
        pred = X @ w
        rel = np.where(y > 0, (pred - y) / np.maximum(y, 1), 0.0)
        # per-file errors on whole files, and leave-one-file-out for the honest figure
        names = np.array([r[0] for r in rows])
        whole = {}
        loo = {}
        for name in files:
            mask = names == name
            full = int(np.where(mask)[0][-1])  # the whole file is the file's last row
            train = ~mask & ok
            wl, _ = nnls(X[train] / y[train][:, None], np.ones(train.sum()))
            whole[name] = float((X[full] @ w - y[full]) / y[full])
            loo[name] = float((X[full] @ wl - y[full]) / y[full])
        result[key] = {
            "weights_milli": {n: round(float(v) * 1000) for n, v in zip(FEATURES, w)},
            "fit_error_files": whole,
            "loo_error_files": loo,
            "mean_abs_fit": float(np.mean([abs(v) for v in whole.values()])),
            "mean_abs_loo": float(np.mean([abs(v) for v in loo.values()])),
            "max_abs_loo": float(max(abs(v) for v in loo.values())),
        }
    chars4 = {}
    for key, count in counters.items():
        errs = []
        for name in files:
            with open(os.path.join(CORPUS, name), encoding="utf-8", newline="") as f:
                text = f.read()
            errs.append(abs((-(-len(text) // 4)) - count(text)) / count(text))
        chars4[key] = {"mean_abs": float(np.mean(errs)), "max_abs": float(max(errs))}
    print(json.dumps({"families": result, "chars4": chars4}, indent=1))


if __name__ == "__main__":
    main()

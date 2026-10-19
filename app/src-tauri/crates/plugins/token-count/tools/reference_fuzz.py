"""Writes a file of random texts with the reference counts, for the differential test of the splitters and the merge.

    python tools/reference_fuzz.py <folder with the two .tiktoken files> <out.jsonl> [count] [seed]

Then:  PERMARK_TOKEN_VOCAB_DIR=<folder> PERMARK_TOKEN_FUZZ_FILE=<out.jsonl> cargo test -p permark-plugin-token-count fuzz

Each line: {"text": ..., "o200k": n, "cl100k": n}. The alphabet is weighted toward the characters the splitting rules treat
specially: apostrophes, newlines and carriage returns, slashes, the Unicode spaces, titlecase | modifier | other letters,
combining marks, digits of other scripts, controls, private use, emoji and unassigned code points.
"""

import json
import os
import random
import sys

import tiktoken
import tiktoken_ext.openai_public as public
from tiktoken.load import load_tiktoken_bpe

ALPHABET = (
    list("abcdefghijklmnopqrstuvwxyz") * 3
    + list("ABCDEFGHIJKLMNOPQRSTUVWXYZ") * 2
    + list("0123456789") * 2
    + [" "] * 12
    + ["\n"] * 5
    + ["\r"] * 3
    + ["\t"] * 2
    + ["'"] * 4
    + ["/"] * 2
    + list("!\"#$%&()*+,-.:;<=>?@[\\]^_`{|}~")
    + list("sStTdDmMlLvVrReE")
    + ["ſ", "’", " ", " ", " ", "　", "​", " ", "\u0085", "\u000b", "\u000c"]
    + list("éèàçñüöäßÉÈÀÇÑÜÖÄ")
    + ["ǅ", "ǈ", "ǲ", "ᾈ"]
    + ["ʰ", "ˆ", "々", "ー"]
    + list("你好世界こんにちは안녕하세요")
    + list("مرحباשלוםПриветΓεια")
    + ["́", "̈", "͡", "ि", "्", "้"]
    + ["٣", "१", "Ⅷ", "½", "①"]
    + ["\u0000", "\u0001", "\u001f", "\u007f", "\u0080", "\u009f"]
    + ["", "\U000f0000", "͸", "\U000e0080", "�", "﻿"]
    + ["\U0001f600", "\U0001f680", "✅", "‍", "️", "\U0001f1e7", "\U00010400", "\U00020000"]
)


def load(folder):
    out = {}
    original = public.load_tiktoken_bpe
    for key, name in (("o200k", "o200k_base"), ("cl100k", "cl100k_base")):
        path = os.path.join(folder, name + ".tiktoken")
        public.load_tiktoken_bpe = lambda url, expected_hash=None, path=path: load_tiktoken_bpe(path)
        spec = getattr(public, name)()
        out[key] = tiktoken.Encoding(spec["name"], pat_str=spec["pat_str"], mergeable_ranks=spec["mergeable_ranks"],
                                     special_tokens=spec["special_tokens"])
    public.load_tiktoken_bpe = original
    return out


def main():
    folder, out = sys.argv[1], sys.argv[2]
    count = int(sys.argv[3]) if len(sys.argv) > 3 else 20000
    seed = int(sys.argv[4]) if len(sys.argv) > 4 else 1
    rnd = random.Random(seed)
    encodings = load(folder)
    with open(out, "w", encoding="utf-8", newline="\n") as f:
        for _ in range(count):
            n = rnd.choice([1, 2, 3, 5, 8, 13, 21, 40, 80, 200])
            text = "".join(rnd.choice(ALPHABET) for _ in range(rnd.randint(1, n)))
            row = {"text": text}
            for key, enc in encodings.items():
                row[key] = len(enc.encode_ordinary(text))
            f.write(json.dumps(row, ensure_ascii=True) + "\n")


if __name__ == "__main__":
    main()

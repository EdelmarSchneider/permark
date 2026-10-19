"""Writes the expected token counts the exactness tests compare against.

The reference is the `tiktoken` package (`Encoding.encode_ordinary`: special-token text counts as ordinary text),
loaded from the same rank files the plugin downloads.

    python -m pip install tiktoken
    python tools/reference_counts.py <folder with o200k_base.tiktoken and cl100k_base.tiktoken> [<Gemini tokenizer.model>]

Run it from the plugin crate folder. It rewrites `corpus/expected.json` (one count per corpus file and encoding; with a
SentencePiece model also `gemini`, the count of the tokenizer Google's SDK uses for Gemini locally: it only measures the
`gemini` estimator, nothing exact is claimed for it) and `corpus/cases.json` (short texts that exercise the splitting
rules, with their counts).
"""

import json
import os
import sys

import tiktoken
import tiktoken_ext.openai_public as public
from tiktoken.load import load_tiktoken_bpe

HERE = os.path.dirname(os.path.abspath(__file__))
CORPUS = os.path.join(HERE, "..", "corpus")
ENCODINGS = {"o200k": "o200k_base", "cl100k": "cl100k_base"}

CASES = {
    "empty": "",
    "one letter": "a",
    "one space": " ",
    "hello": "hello world",
    "sentence": "The quick brown fox jumps over the lazy dog.",
    "capitals": "HELLO WORLD",
    "camel": "helloWorld HelloWorld HTTPServer parseXMLFile",
    "contractions": "I'm sure it's fine; they'll say we've done what you'd do, and she'S not here. DON'T. 'tis 'twas",
    "long s contraction": "it\u2019s it's it\u017f",
    "digits": "1 22 333 4444 55555 666666 7777777 1234567890123456789",
    "digits and letters": "abc123def 12ab34 3.14159 1,000,000 0xFF",
    "punctuation": "!!! ... ??? --- *** ### /// \\\\\\ ((( ))) {{{ }}} <<< >>>",
    "punct then newline": "foo(); \nbar();\n\nbaz /\n/ qux",
    "slash after newline": "end of line\n/ next",
    "newlines": "a\nb\r\nc\rd\n\n\ne\r\n\r\nf",
    "only newlines": "\n\n\n",
    "trailing spaces": "text with trailing spaces   ",
    "trailing newline spaces": "text\n  ",
    "leading spaces": "   indented",
    "space runs": "a  b   c    d     e",
    "tabs": "a\tb\t\tc\n\td",
    "whitespace only": "   \t  \n  \t ",
    "whitespace then text": "  \n  text",
    "whitespace middle": "one \n \n two",
    "nbsp and unicode spaces": "a\u00a0b\u2003c\u2009d\u3000e\u200bf\u2028g\u0085h",
    "emoji": "Hello \U0001f600 world \U0001f680\U0001f525 \u2705 \u274c \U0001f469\u200d\U0001f4bb \U0001f1e7\U0001f1f7",
    "cjk": "\u4f60\u597d\uff0c\u4e16\u754c\u3002\u3053\u3093\u306b\u3061\u306f\u4e16\u754c\u3002\uc548\ub155\ud558\uc138\uc694",
    "combining marks": "e\u0301 a\u0308 Z\u0361a\u035fl\u0360g\u0346o\u0357 \u0915\u094d\u0937\u093f",
    "marks after space": " \u0301abc \u0301\u0301",
    "rtl": "\u0645\u0631\u062d\u0628\u0627 \u0628\u0627\u0644\u0639\u0627\u0644\u0645 \u05e9\u05dc\u05d5\u05dd \u05e2\u05d5\u05dc\u05dd hello",
    "cyrillic and greek": "\u041f\u0440\u0438\u0432\u0435\u0442, \u043c\u0438\u0440! \u0393\u03b5\u03b9\u03b1 \u03c3\u03bf\u03c5 \u03ba\u03cc\u03c3\u03bc\u03b5",
    "titlecase letters": "\u01c5 \u01c8 \u01cb \u01f2 \u1f88 \u01c5ellow",
    "modifier and other letters": "\u02b0\u02b1abc \u05d0\u05d1\u05d2 \u0e01\u0e02\u0e04 \u3042\u3044\u3046 \u30a2\u30a4\u30a6",
    "uppercase then lowercase": "ABCdef ABCDef AbCdEf \u00c9COLE \u00c9cole",
    "accents": "caf\u00e9 na\u00efve fa\u00e7ade Z\u00fcrich \u00fcber \u00d1and\u00fa",
    "special token text": "<|endoftext|> <|fim_prefix|> <|endofprompt|> <|im_start|>",
    "url": "https://example.org/a/b?c=d&e=f#g",
    "path": "C:\\Users\\name\\file.txt /usr/local/bin/tool ./relative/../path",
    "code": "fn main() {\n    let x = vec![1, 2, 3];\n    println!(\"{x:?}\");\n}\n",
    "json": "{\"a\": [1, 2, {\"b\": null}], \"c\": \"d\"}",
    "markdown": "# Title\n\n- item **bold** and `code`\n- [link](http://x.y)\n\n> quote\n\n| a | b |\n|---|---|\n| 1 | 2 |\n",
    "long word": "a" * 300,
    "long digits": "7" * 100,
    "long punct": "=" * 200,
    "long mixed no spaces": "abcdefghijklmnopqrstuvwxyz0123456789" * 12,
    "base64": "VGhpcyBpcyBhIGJhc2U2NCBlbmNvZGVkIHN0cmluZyB1c2VkIGZvciBhIHRlc3QgY2FzZSBvZiB0aGUgY291bnRlcg==" * 3,
    "repeated word": "word " * 500,
    "repeated pair": "ab" * 400,
    "invalid utf8 stand-in": "\ufffd\ufffd text \ufffd",
    "nul and controls": "a\u0000b\u0001c\u001fd\u007fe",
    "private use": "\ue000\ue001 \U000f0000",
    "unassigned": "\u0378\u0379 \U000e0080",
    "supplementary letters": "\U00010400\U00010428 \U0001d400 \U00020000\U00020001",
    "numbers other": "\u0663\u0664 \u2460\u2461 \u00bd \u2167 \u0967\u0968\u0969\u096a",
}


def load(folder):
    out = {}
    original = public.load_tiktoken_bpe
    for key, name in ENCODINGS.items():
        path = os.path.join(folder, name + ".tiktoken")
        public.load_tiktoken_bpe = lambda url, expected_hash=None, path=path: load_tiktoken_bpe(path)
        spec = getattr(public, name)()
        out[key] = tiktoken.Encoding(
            name=spec["name"],
            pat_str=spec["pat_str"],
            mergeable_ranks=spec["mergeable_ranks"],
            special_tokens=spec["special_tokens"],
        )
    public.load_tiktoken_bpe = original
    return out


def count(encoding, text):
    return len(encoding.encode_ordinary(text))


def main():
    folder = sys.argv[1]
    encodings = load(folder)
    gemini = None
    if len(sys.argv) > 2:
        import sentencepiece

        gemini = sentencepiece.SentencePieceProcessor(model_file=sys.argv[2])
    files = sorted(f for f in os.listdir(CORPUS) if f not in ("expected.json", "cases.json", "README.md"))
    expected = {"tiktoken": tiktoken.__version__ if hasattr(tiktoken, "__version__") else "unknown", "files": {}}
    for name in files:
        with open(os.path.join(CORPUS, name), encoding="utf-8", newline="") as f:
            text = f.read()
        expected["files"][name] = {key: count(enc, text) for key, enc in encodings.items()}
        if gemini is not None:
            expected["files"][name]["gemini"] = len(gemini.encode(text))
        expected["files"][name]["chars"] = len(text)
    with open(os.path.join(CORPUS, "expected.json"), "w", encoding="utf-8", newline="\n") as f:
        json.dump(expected, f, indent=1, ensure_ascii=True)
        f.write("\n")
    cases = [
        {"name": name, "text": text, **{key: count(enc, text) for key, enc in encodings.items()}}
        for name, text in CASES.items()
    ]
    with open(os.path.join(CORPUS, "cases.json"), "w", encoding="utf-8", newline="\n") as f:
        json.dump(cases, f, indent=1, ensure_ascii=True)
        f.write("\n")
    print(len(files), "files,", len(cases), "cases")


if __name__ == "__main__":
    main()

"""Writes `fixtures/estimate.json`: texts with the estimate of every family, computed here from the weights below.

    python tools/estimate_fixtures.py

Run it from the plugin crate folder. The Rust estimator and its TypeScript mirror must reproduce every number
(integer arithmetic only), so a class table or a weight that drifts in one of the three fails a test. The weights are
the ones printed by `tools/calibrate.py` (rounded to milli-tokens); `claude` is the `o200k` set times `K`.
"""

import importlib.util
import json
import os

HERE = os.path.dirname(os.path.abspath(__file__))
spec = importlib.util.spec_from_file_location("calibrate", os.path.join(HERE, "calibrate.py"))
calibrate = importlib.util.module_from_spec(spec)
spec.loader.exec_module(calibrate)

O200K = {"space": 118, "newline": 387, "lower": 0, "upper": 81, "digit": 515, "latinExt": 730, "cjk": 875,
         "greekCyr": 324, "letterOther": 403, "emoji": 2616, "symbol": 1224, "punct": 514, "word": 861, "excess": 582}
CL100K = {"space": 124, "newline": 421, "lower": 0, "upper": 47, "digit": 521, "latinExt": 1833, "cjk": 1338,
          "greekCyr": 639, "letterOther": 1021, "emoji": 4357, "symbol": 595, "punct": 455, "word": 900, "excess": 640}
GEMINI = {"space": 123, "newline": 916, "lower": 30, "upper": 161, "digit": 1043, "latinExt": 532, "cjk": 757,
          "greekCyr": 300, "letterOther": 416, "emoji": 2706, "symbol": 632, "punct": 675, "word": 714, "excess": 436}
# K x 1000 / 1000: chosen so the English prose sample (corpus/en-novel.txt) comes out at 3.5 characters per token.
K_PER_MILLE = 1070
CLAUDE = {name: round(value * K_PER_MILLE / 1000) for name, value in O200K.items()}


def estimate(text, weights):
    counts = calibrate.features(text)
    milli = sum(weights[name] * count for name, count in zip(calibrate.FEATURES, counts))
    return -(-milli // 1000)


def chars4(text):
    return -(-len(text) // 4)


TEXTS = {
    "empty": "",
    "word": "hello",
    "sentence": "The quick brown fox jumps over the lazy dog.",
    "long words": "internationalization antidisestablishmentarianism supercalifragilistic",
    "camel and snake": "parseXMLFile some_snake_case_name HTTPServer2Go",
    "digits": "1 22 333 4444 55555 3.14159 2026-10-19 +55 (11) 91234-5678",
    "accents": "café naïve façade Zürich über Ñandú",
    "cjk": "你好，世界。こんにちは世界。안녕하세요",
    "greek and cyrillic": "Привет, мир! Γεια σου κόσμε",
    "other scripts": "مرحبا بالعالم שלום עולם नमस्ते สวัสดี",
    "emoji": "Hello 😀 world 🚀🔥 ✅ ❌ 👩‍💻 🇧🇷 ❤️",
    "symbols": "∑ ∏ ∫ √ ∞ ≈ ≠ → ← ┌─┬─┐ “quotes” — … ©",
    "ascii punctuation": "!!! ... ??? --- *** ### /// {{{ }}} <<< >>> $%&@^~|\\`",
    "spaces": "a  b   c\t\td e f　g",
    "newlines": "a\nb\r\nc\rd e\u0085f\u000bg\u000ch",
    "controls": "a\u0000b\u0001c\u007fd\u0080e\u009ff",
    "private and unassigned": " \U000f0000 ͸ \U000e0080",
    "supplementary": "\U00010400 \U0001d400 \U00020000 \U0002a6d6 \U0003134a",
    "marks": "é ä Z͡a͟l͠g͆o͗",
    "fullwidth": "ＡＢＣ１２３　全角",
    "code": "fn main() {\n    let x = vec![1, 2, 3];\n    println!(\"{x:?}\");\n}\n",
    "long run": "a" * 300,
    "word boundary": "abcdef abcdefg abcdefgh ab-cd ab_cd ab1cd 12ab",
}

# corpus files: first 600 characters of each, and two whole small files
CORPUS = os.path.join(HERE, "..", "corpus")


def main():
    items = [{"name": name, "text": text} for name, text in TEXTS.items()]
    for name in sorted(os.listdir(CORPUS)):
        if name in ("expected.json", "cases.json", "README.md"):
            continue
        with open(os.path.join(CORPUS, name), encoding="utf-8", newline="") as f:
            text = f.read()
        items.append({"name": "corpus " + name + " (head)", "text": text[:600]})
    for name in ("en-novel.txt", "zh-hant-novel.txt", "code.rs"):
        with open(os.path.join(CORPUS, name), encoding="utf-8", newline="") as f:
            items.append({"name": "corpus " + name, "text": f.read()})
    for item in items:
        t = item["text"]
        item["estimates"] = {
            "generic": estimate(t, O200K),
            "o200k": estimate(t, O200K),
            "cl100k": estimate(t, CL100K),
            "gemini": estimate(t, GEMINI),
            "claude": estimate(t, CLAUDE),
            "chars4": chars4(t),
        }
    out = os.path.join(HERE, "..", "fixtures", "estimate.json")
    os.makedirs(os.path.dirname(out), exist_ok=True)
    with open(out, "w", encoding="utf-8", newline="\n") as f:
        json.dump({"claude": CLAUDE, "cases": items}, f, indent=1, ensure_ascii=True)
        f.write("\n")
    en = next(i for i in items if i["name"] == "corpus en-novel.txt")
    print("claude weights", CLAUDE)
    print("en-novel chars per token (claude)", len(en["text"]) / en["estimates"]["claude"])
    print(len(items), "cases")


if __name__ == "__main__":
    main()

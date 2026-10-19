# Token count corpus

Sample texts the token count plugin is tested and calibrated on. The files are committed byte for byte (`.gitattributes`:
no line-ending conversion); the counts next to them come from the tokenizers' own reference code, not from this program.

| Files | Source |
|---|---|
| `en-novel.txt`, `pt-novel.txt`, `es-novel.txt`, `de-novel.txt`, `it-novel.txt`, `fr-novel.txt`, `zh-hant-novel.txt`, `ja-novel.txt` | Excerpts of public-domain books from Project Gutenberg: Pride and Prejudice, Dom Casmurro, Don Quijote, Die Verwandlung, I promessi sposi, Les misérables, 紅樓夢, 羅生門. Line breaks as in the source. |
| `ru-prose.txt`, `ar-prose.txt`, `he-prose.txt`, `el-prose.txt`, `ko-prose.txt`, `hi-prose.txt`, `th-prose.txt`, `zh-hans-text.txt` | Sentences written for this corpus, one script each. |
| `en-markdown.md`, `code.rs`, `code.ts`, `code.svelte`, `code.css`, `data.json`, `config.toml`, `config.yml` | Copies of files of this repository (Markdown, Rust, TypeScript, Svelte, CSS, JSON, TOML, YAML). |
| `code.py`, `code.sql`, `code.html`, `code.sh` | Short samples written for this corpus. |
| `data.csv`, `data.log`, `data.base64`, `data.hex`, `data.urls` | Generated with a fixed seed: tables, log lines, random bytes as base64 and as a hex dump, addresses. |
| `numbers.txt`, `symbols.txt`, `whitespace.txt` | Digits, dates and units; emoji, combining marks, math and box drawing, odd spaces; indentation and trailing whitespace. |

## The expected counts

- `expected.json`: per file, the count of each exact encoding (`o200k`, `cl100k`) from the `tiktoken` package
  (`Encoding.encode_ordinary`), and `gemini` from the SentencePiece tokenizer Google's SDK uses to count Gemini tokens
  locally. `gemini` only measures the estimator of that name; nothing exact is claimed for it.
- `cases.json`: short texts that exercise the splitting rules (apostrophes, newlines and slashes, trailing whitespace,
  emoji, CJK, combining marks, titlecase letters, controls, very long runs ...), with the same counts.
- `../tools/reference_counts.py` writes both files: `python tools/reference_counts.py <folder with the rank files>
  [<tokenizer.model of Gemini>]`. `../tools/reference_fuzz.py` makes random texts with their reference counts for the
  differential test (`fuzz_against_the_reference`).
- `../tools/calibrate.py` fits the estimators' weights on this corpus; `../tools/estimate_fixtures.py` writes
  `../fixtures/estimate.json`, the numbers the Rust and the TypeScript estimators must reproduce.

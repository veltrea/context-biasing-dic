# biasdiff

**Read this in other languages:** [日本語](README.ja.md)

A diff-based collector of **homophone confusion words** for building ASR
context-biasing dictionaries.

You read a sentence you already have the correct text for, let your ASR
transcribe it, and `biasdiff` diffs the two at the morpheme level. Among the
mismatches it keeps **only the pairs whose readings match** — i.e. genuine
homophone collisions (機械 / 機会, 意思 / 医師) — and discards plain
mis-recognitions caused by articulation or noise. The kept words are the raw
material for the "finishing layer" of a context-biasing dictionary: the small
set of words that pins down which homophone the recognizer should emit.

Because the correct text is in hand, detecting errors is a *comparison*, not an
*inference*: no frontier model and no local LLM are needed.

## How it works

1. Tokenize both the reference (correct) text and the ASR hypothesis with a
   morphological analyzer (Lindera), attaching a reading (kana) to each token.
2. Take a Myers diff over the **token surfaces** and pull out the *replacement*
   blocks (insertions/deletions are usually segmentation drift, so they are
   skipped).
3. For each replacement, normalize and compare the readings of both sides.
   - **Readings match** → homophone collision → kept as a danger word.
   - **Readings differ** → articulation/noise error → dropped to a separate log.
4. Count the kept words by their reference-side surface and emit a plain list.

Everything runs locally. No data leaves the machine, and the output is **words
only** — never the source sentences or any context.

## Build

Requires a recent stable Rust toolchain.

```sh
# Default: IPADIC dictionary, embedded into the binary (lightweight, fast build).
cargo build --release

# Higher reading accuracy with UniDic (pronunciation form).
# The dictionary is large, so the first build downloads and compiles for a while.
cargo build --release --no-default-features --features unidic
```

The binary is at `target/release/biasdiff`. Optionally install it:

```sh
cargo install --path .
```

### Dictionary choice

| Feature            | Dictionary | Reading source            | Build cost            |
| ------------------ | ---------- | ------------------------- | --------------------- |
| `ipadic` (default) | IPADIC     | reading field             | light, fast           |
| `unidic`           | UniDic     | pronunciation-form output | heavy download + build |

UniDic gives more accurate readings (it carries the pronunciation form), which
helps the homophone filter. IPADIC is plenty to start with.

## Usage

### Batch: compare two files line by line

`--reference` and `--hypothesis` are paired line by line (line *i* against line
*i*).

```sh
biasdiff batch \
  --reference ref.txt \
  --hypothesis hyp.txt \
  --output dict.txt \
  --reject reject.txt
```

- The danger-word list goes to `--output` (or stdout if omitted), one word per
  line, sorted by frequency.
- `--counts` appends the count as `word\tcount`.
- `--format <txt|counts|amical-json>` picks the output shape (default `txt`).
  `amical-json` writes a meta-tagged dictionary for the Amical voice-input app,
  and `--field <label>` sets its field tag (default `general`). `--counts` is
  kept for compatibility and equals `--format counts`; an explicit `--format`
  wins when both are given.
- Rejected (reading-mismatch) pairs go to `--reject` for later trend analysis.
- A one-line summary is printed to stderr, so piping stdout gives you just the
  list.

For the Amical dictionary, select `amical-json` and name the field:

```sh
biasdiff batch \
  --reference ref.txt \
  --hypothesis hyp.txt \
  --format amical-json \
  --field dev \
  -o dev.biasing.json
```

### Repl: the minimal read-aloud loop

Enter a reference sentence, then its ASR result, and see the diff immediately.
Repeat. An empty line or Ctrl-D finishes.

```sh
biasdiff repl --output dict.txt
```

`[+]` marks a kept homophone, `[-]` marks a dropped reading-mismatch. With
`--output`, the list is re-saved after every pair so an interrupted session is
not lost.

### Common options

| Option            | Effect                                                        |
| ----------------- | ------------------------------------------------------------- |
| `--dict <ipadic\|unidic>` | Pick among the dictionaries embedded at build time.   |
| `--strict`        | Disable reading-yure folding; require exact reading match.    |
| `--format <txt\|counts\|amical-json>` | Output shape (default `txt`). `amical-json` → Amical dictionary. |
| `--field <label>` | Field tag for `amical-json` output (default `general`).       |
| `--counts`        | (batch/repl) Emit `word\tcount`; same as `--format counts`.  |

By default, readings are folded for long vowels, geminate (sokuon), dakuten,
and small kana so that spelling variants of the same sound still match. `--strict`
turns that off.

## Output format

- **Danger-word list** — one reference-side word per line (with `--counts`,
  `word<TAB>count`). This is ready to feed into an ASR term list.
- **Amical biasing dictionary (JSON)** — with `--format amical-json`, a single
  JSON object the Amical voice-input app imports directly:

  ```json
  {
    "schema": "amical-biasing-dictionary",
    "version": 1,
    "field": "dev",
    "generator": "biasdiff 0.1.0",
    "terms": [
      { "word": "機械", "count": 12 },
      { "word": "意思", "count": 5 }
    ]
  }
  ```

  `terms` is ordered by `count` descending (ties broken by `word` ascending) —
  the same order as the plain list. Amical concatenates the words from the front
  and truncates the tail to fit its context budget, so the most frequent words —
  the ones most worth biasing — survive. Japanese is emitted as raw UTF-8 (never
  `\uXXXX`), pretty-printed, with one trailing newline; an empty collection still
  yields valid JSON with `"terms": []`.
- **Reject log** — `reference<TAB>hypothesis<TAB>ref-reading<TAB>hyp-reading`,
  words and readings only (no sentences).

## Privacy

- Input is only the example sentences you chose yourself.
- Diffing, tokenization, and reading lookup all happen locally; there is no
  network path.
- Output is words only — the original sentences cannot be reconstructed from it.

## Scope

This tool targets the **general** homophone danger words of the finishing
layer. Idiosyncratic, one-off mis-conversions are meant to be added by hand
during daily use. Exhaustiveness is a non-goal; stop adding words once the ASR
improvement curve flattens.

## Design

See the design document (Japanese):
[biasing-dict-diff-utility-design.md](biasing-dict-diff-utility-design.md).

## License

Licensed under either of Apache-2.0 or MIT, at your option.

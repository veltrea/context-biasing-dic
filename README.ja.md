# biasdiff

**他の言語で読む:** [English](README.md)

ASR のコンテキストバイアシング辞書を作るための、**同音衝突する危険語**を
diff で集めるユーティリティです。

正解テキストを手元に用意して読み上げ、ASR に書き起こさせ、`biasdiff` が両者を
形態素単位で diff します。食い違いのうち**読みが一致するペアだけ**——つまり
本物の同音衝突（機械 / 機会、意思 / 医師）——を残し、滑舌やノイズ由来の素の
誤認識は捨てます。残った語が、コンテキストバイアシング辞書の「仕上げ層」、
すなわち同音異義語の出し分けを確定させる小さな語の集合の素材になります。

正解が手元にある以上、誤りの検出は*推論*ではなく*照合*で済みます。
フロンティアモデルもローカル LLM も要りません。

## 仕組み

1. 正解文と ASR 認識結果の両方を形態素解析器（Lindera）でトークン化し、各
   トークンに読み（カナ）を付ける。
2. **トークンの表記**列に対して Myers diff を取り、*置換*ブロックを取り出す
   （挿入・削除は区切りのズレが多いので採らない）。
3. 各置換について、両側の読みを正規化して突き合わせる。
   - **読みが一致** → 同音衝突 → 危険語として採用。
   - **読みが違う** → 滑舌・ノイズ由来 → 別ログへ落とす。
4. 採用した語を正解側の表記で集計し、プレーンなリストとして書き出す。

すべてローカルで完結します。データは外に出ず、出力は**語のみ**——元の文や
文脈は一切含みません。

## ビルド

新しめの安定版 Rust ツールチェインが必要です。

```sh
# 既定: IPADIC 辞書をバイナリに埋め込む（軽量・ビルドが速い）。
cargo build --release

# UniDic で読み精度を上げる（発音形を使う）。
# 辞書が大きいため、初回ビルドはダウンロードとコンパイルに時間がかかる。
cargo build --release --no-default-features --features unidic
```

バイナリは `target/release/biasdiff` にできます。必要ならインストール:

```sh
cargo install --path .
```

### 辞書の選択

| Feature            | Dictionary | Reading source            | Build cost            |
| ------------------ | ---------- | ------------------------- | --------------------- |
| `ipadic` (default) | IPADIC     | reading field             | light, fast           |
| `unidic`           | UniDic     | pronunciation-form output | heavy download + build |

UniDic は読み（発音形）がより正確で、同音フィルタの精度が上がります。まずは
IPADIC で十分です。

## 使い方

### batch: 2 ファイルを行対応で照合

`--reference` と `--hypothesis` は行ごとに対応づけられます（i 行目どうし）。

```sh
biasdiff batch \
  --reference ref.txt \
  --hypothesis hyp.txt \
  --output dict.txt \
  --reject reject.txt
```

- 危険語リストは `--output`（省略時は標準出力）へ、1 行 1 語、頻度順で出ます。
- `--counts` を付けると `word\tcount` の形で回数を付けます。
- `--format <txt|counts|amical-json>` で出力形式を選びます（既定 `txt`）。
  `amical-json` は音声入力アプリ Amical がそのまま取り込めるメタ付き辞書を書き出し、
  `--field <label>` でその分野ラベルを指定します（既定 `general`）。`--counts` は
  後方互換として残り `--format counts` と同等です。両方を指定したときは `--format` を優先します。
- 除外（読み不一致）ペアは `--reject` へ。後で傾向分析に使えます。
- サマリは標準エラーへ出るので、標準出力をパイプすればリストだけが取れます。

Amical 用辞書を出すときは `amical-json` を選び、分野名を付けます:

```sh
biasdiff batch \
  --reference ref.txt \
  --hypothesis hyp.txt \
  --format amical-json \
  --field dev \
  -o dev.biasing.json
```

### repl: 読み上げの最小ループ

正解文 → その ASR 結果の順に入力すると、即座に diff が見えます。これを
繰り返します。空行か Ctrl-D で終了します。

```sh
biasdiff repl --output dict.txt
```

`[+]` は採用した同音語、`[-]` は落とした読み不一致です。`--output` を付けると
1 ペアごとにリストを保存し直すので、途中で中断しても失われません。

### よく使うオプション

| Option            | Effect                                                        |
| ----------------- | ------------------------------------------------------------- |
| `--dict <ipadic\|unidic>` | ビルド時に埋め込んだ辞書から選ぶ。                    |
| `--strict`        | 読みゆれの畳み込みを無効化し、読みの完全一致を要求する。      |
| `--format <txt\|counts\|amical-json>` | 出力形式（既定 `txt`）。`amical-json` は Amical 用辞書。 |
| `--field <label>` | `amical-json` 出力の分野ラベル（既定 `general`）。           |
| `--counts`        | (batch/repl) `word\tcount` を出力。`--format counts` と同等。 |

既定では、長音・促音・濁点・小書き仮名のゆれを畳み込み、同じ音の表記ゆれでも
一致するようにします。`--strict` でこれを切ります。

## 出力フォーマット

- **危険語リスト** — 1 行 1 語（正解側の表記）。`--counts` 付きなら
  `word<TAB>count`。そのまま ASR の用語集に投入できます。
- **Amical バイアシング辞書（JSON）** — `--format amical-json` を付けると、音声入力
  アプリ Amical がそのまま取り込める 1 個の JSON オブジェクトを出します:

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

  `terms` は count 降順（同数は word 昇順）——プレーンなリストと同じ並びです。
  Amical は語を先頭から連結し、文脈予算に収まるよう末尾を切り捨てるため、頻度の高い
  ＝バイアスをかける価値の高い語ほど生き残ります。日本語は `\uXXXX` にせず生の UTF-8、
  pretty 整形、末尾改行 1 個で出します。危険語が 0 件でも `"terms": []` の妥当な JSON になります。
- **除外ログ** — `reference<TAB>hypothesis<TAB>ref-reading<TAB>hyp-reading`。
  語と読みのみ（文は含めない）。

## プライバシー

- 入力は自分で選んだ例文だけ。
- diff・形態素解析・読み付与はすべてローカルで完結し、外部送信経路を持たない。
- 出力は語のみ——そこから元の文を復元することはできない。

## 守備範囲

このツールが狙うのは、仕上げ層のうち**一般的な**同音危険語です。固有の
一回性の誤変換は、日常運用の中で一語ずつ足していく前提です。網羅は目的に
せず、ASR の改善カーブが寝たら語の追加を止めます。

## 設計

設計ドキュメント（日本語）:
[biasing-dict-diff-utility-design.md](biasing-dict-diff-utility-design.md)。

## ライセンス

Apache-2.0 または MIT のいずれか、利用者の選択でライセンスされます。

# biasdiff harvest/evaluate 実装計画書

対応する仕様は [SPEC.ja.md](SPEC.ja.md)（英語版 [SPEC.md](SPEC.md)）。
本書は「何を・どの順で・どう検証して」作るかの実行計画である。
v0.1 設計書の流儀（10 章「最小ループを回しながら必要な段だけ足す」）を踏襲し、
各 Step は**動く縦穴**を出口にする。

| | |
| --- | --- |
| 状態 | Draft |
| 日付 | 2026-06-10 |
| 言語 | 本書は日本語のみ（SPEC は英日ペア） |

## 0. 前提（2026-06-10 時点の実機確認）

| 道具 | 状態 | 備考 |
| --- | --- | --- |
| VOICEVOX | **稼働中** 0.25.1（`127.0.0.1:50021/version` で確認） | 主力 TTS |
| macOS `say` | 日本語 9 話者（Kyoko, Eddy, Flo ほか） | フォールバック TTS |
| ffmpeg | あり | 16 kHz mono WAV への正規化 |
| mlx-audio（Qwen3-ASR） | **未導入** — 唯一の不足 | Step 0 で導入 |
| Rust toolchain | あり（v0.1 がビルド済み） | |

開発機で Step 0〜4 を完結させ、夜間運用（Step 5）の段階でリモートの
Apple Silicon 機（Mac mini M4）へ実行を移す。

## 1. ロードマップ

| Step | 内容 | 規模 | 依存 | 出口（完了条件） |
| --- | --- | --- | --- | --- |
| 0 | 配管検証（コードを書く前の手動 1 往復） | S | なし | TTS→ASR→diff が通り、同音衝突が観測できる |
| 1 | トレイト 3 種 + 最小 `harvest`（FileSource） | M | 0 | file→TTS→ASR→コア→出力が 1 コマンドで通り、再実行が冪等 |
| 2 | `extract` + QiitaSource / ZennSource | M | 1 | `--dry-run` の例文が目視で実用水準 |
| 3 | 多声マトリクス + 投票 | S | 1 | 投票が偽陽性を削った実例を記録 |
| 4 | `evaluate` | M | 1, Q1 | 推奨 N と curve.tsv が再現可能に出る |
| 5 | 夜間自動回し | S | 2〜4 | 朝に辞書差分とカーブが増えている |

規模感: S = 半日枠、M = 数日枠（時間はユーザーのペースで）。

## 2. Step 0 — 配管検証（コードを書く前に）

**目的**: (a) ツール鎖の疎通確認。(b) SPEC 未決事項 Q3（TTS 音声からも同音衝突が
出るか＝転移性）の最初の実測。設計の最大の賭けを最初に測る。

```sh
# 1. mlx-audio を導入（uv の仮想環境。導入したバージョンを記録する）
uv venv ~/.venvs/mlx-audio
source ~/.venvs/mlx-audio/bin/activate
uv pip install mlx-audio
python -m mlx_audio.stt.generate --help   # エントリポイント表記はここで必ず確認

# 2. VOICEVOX で 1 文を合成（話者一覧は GET /speakers で確認できる）
TEXT="機械学習で意思決定を支援する仕組みを実装する"
ENC=$(python3 -c "import urllib.parse,sys;print(urllib.parse.quote(sys.argv[1]))" "$TEXT")
curl -s -X POST "http://127.0.0.1:50021/audio_query?speaker=3&text=$ENC" -o q.json
curl -s -X POST "http://127.0.0.1:50021/synthesis?speaker=3" \
     -H 'Content-Type: application/json' -d @q.json -o sent.wav

# 3. 認識層の入力契約（16 kHz mono WAV）へ正規化
ffmpeg -y -i sent.wav -ar 16000 -ac 1 sent16k.wav

# 4. Qwen3-ASR で認識
python -m mlx_audio.stt.generate \
  --model mlx-community/Qwen3-ASR-0.6B-8bit --audio sent16k.wav

# 5. 既存 repl に貼って diff（正解文 → 認識結果の順）
cargo run --release -- repl
```

例文には同音衝突を起こしやすい語（機械/機会・意思/医師・実装/失踪）を
仕込んである。

**完了条件 / 観測すること**:
- 鎖全体が通る。各段の所要時間をメモする（以後の規模見積もりの基礎）。
- `[+]`（同音衝突）が観測できれば Q3 に好材料。1 文で出なければ 10 文程度に
  増やしてから判断する。
- 0.6B の認識が粗ければ `1.7B-8bit` でも試す（Q2 の事前観測）。

**リスク**: mlx-audio の CLI 表記ゆれ。`--help` で確認し、確定した呼び出し方を
本書のこの節に追記して更新する。

## 3. Step 1 — トレイト 3 種 + 最小 `harvest`（FileSource）

**目的**: SPEC 5 章の抽象を導入し、`file → TTS → ASR → 既存コア → 辞書出力` が
1 コマンドで通る最小の縦穴を作る。

**作業**:
- 新規モジュール: `src/source.rs`・`src/synth.rs`・`src/recognize.rs`・
  `src/vote.rs`（この段では空に近くてよい）・`src/harvest.rs`
- アダプタ: `src/source_file.rs`・`src/synth_voicevox.rs`・`src/synth_say.rs`・
  `src/asr_qwen3_mlx.rs`（まず 1 ファイル = 1 プロセスの素朴版でよい。
  SPEC 9 章のバッチドライバ `scripts/qwen3_asr_batch.py` は遅さを実測してから）
- `Cargo.toml` に `harvest` feature 追加（**`default` には足さない**）
- キャッシュ基盤: `audio/`・`asr/` の内容アドレス実装。`.gitignore` に
  `harvest_cache/` を追加（記事本文を含むため必須）
- CLI: `biasdiff harvest --source file --input sentences.txt --tts voicevox --asr qwen3-mlx -o dict.txt`

**検証**:
- unit: モック 3 種（TextSource/Synthesizer/Recognizer）でオーケストレーションを
  テスト。音声もネットも不要で回ることが既存コアと同じ品質基準。
- 統合: `#[ignore]` 付き・`harvest` feature 限定のテスト（VOICEVOX 稼働時のみ
  手動実行）。
- 手動 e2e: 文 10 本のファイルで一周し、**2 回目の実行がキャッシュ命中で
  目に見えて速い**ことを確認。

**完了条件**: 上記 1 コマンドが通り amical-json が出る。再実行が冪等。
あわせて Q2 の実測: 同じ 10 文を 0.6B / 1.7B で回し、所要時間と誤りの傾向を
記録する。

## 4. Step 2 — `extract` + QiitaSource / ZennSource

**目的**: 例文化の品質（＝辞書の純度）を作り込み、実ソースに接続する。

**作業**:
- `src/extract.rs` を**テストファースト**で書く: コードフェンス・インライン
  コード・URL・表・HTML タグ・見出しを含むフィクスチャ → 期待される文列、の
  テーブルテストを先に置く。
- フィルタ実装: 文長 20〜80 字・日本語文字率 ≥ 0.5・漢字密度スコア・
  記事内上限 20 文・正規化ハッシュの重複排除（SPEC 7 章）。
- `src/source_qiita.rs`（ureq・`QIITA_TOKEN` 対応）、`src/source_zenn.rs`
  （1 req/s スロットル・ツール名入り User-Agent・一覧 → 詳細の 2 段取得）。
- キャッシュ: `articles/{source}/{id}.json` + `seen.jsonl`。
- `--dry-run`（例文化で止めて文を表示）。

**検証**:
- `cargo test`（extract は純粋なので網羅的に固める）。
- `biasdiff harvest --source qiita --query "stocks:>=20 tag:rust" --count 5 --dry-run`
  で文の質を目視。Zenn も同様。
- Zenn のリクエスト間隔が 1 秒以上空いていることをログで確認。

**完了条件**: dry-run の文が「読み上げて自然」な比率でおおむね 8 割以上
（目視）。混入したゴミ文はフィクスチャに追加して extract に反映する。

**リスク**: 例文化の質が出ない → 閾値は CLI オプションにせずコード内定数で
調整する（過剰なオプション化をしない）。記事は豊富にあるので「迷ったら捨てる」
方向に倒す。

## 5. Step 3 — 多声マトリクス + 投票

**作業**: `--voices` / `--rates` のマトリクス展開。`src/vote.rs` に
（正解表記, 認識表記）ペア → 観測話者集合の集計と `--min-votes` 閾値を実装し、
`Collector::add` の前段に組み込む。

**検証**: 同じ文セットを 3 話者で回し、1 話者でしか出ない衝突が落ちることを
ログで確認。`--min-votes 1` で従来挙動に戻ることも確認。

**完了条件**: 投票が偽陽性（特定話者の癖由来のペア）を実際に削った例を
最低 1 つ記録できる。

## 6. Step 4 — `evaluate`

**事前タスク（Q1 を閉じる）**: 公式 `QwenLM/Qwen3-ASR` の `context=` が
プロンプトにどう織り込まれるかを実装から読み、`system_prompt` 文字列の
組み立て方（区切り・前置きの要否）を確定して SPEC の Q1 を更新する。

**作業**: `src/evaluate.rs` — N スケジュール（`--step` / `--max-words`）、
`asr-biased/{hash}/{n}.txt` キャッシュ、衝突率の算出（既存分類器を使う）、
頭打ち判定（`--min-delta` / `--patience`）、`curve.tsv` 出力、`--prune`。

**検証**:
- 既知の衝突を含む小セットで `N=0` → `N=top` の衝突率低下を確認する。
- `curve.tsv` をグラフで目視（単調減 → 頭打ちの形になっているか）。

**完了条件**: 推奨 N が出力され、根拠（curve.tsv）が再現可能。

**リスク**: biasing が効かない / 逆効果 → Q1 の書式を再確認 → 1.7B で再試行 →
それでも効かなければ N スケジュールを細かくし「語数を絞るほど効く」帯を探す。

## 7. Step 5 — 夜間自動回し

**作業**: `scripts/nightly-harvest.sh` — リモート機を Wake-on-LAN で起こし、
SSH で `harvest` を実行し、成果物（辞書・curve・キャッシュ差分）を取り込んで
リモート機をシャットダウンする流れ。**マシン固有の宛先・鍵・電源情報は
リポジトリ外（環境変数・ローカル設定）で渡し、リポジトリには汎用スクリプト
だけを置く。** 起動側は開発機の launchd で定時実行。

**検証**: 手動で 1 回流す → 一晩実走。

**完了条件**: 朝に新しい辞書差分とカーブが増えている。

**備考**: GUI（Tauri アプリ）への収集タブ統合は本計画のスコープ外の
任意項目として残す。

## 8. テスト戦略（全体）

| 層 | 方法 |
| --- | --- |
| 純粋層（`extract` / `vote` / `source` の型） | 通常の `cargo test`。フィクスチャ駆動で網羅的に |
| オーケストレーション（`harvest` / `evaluate`） | トレイト 3 種のモックで unit テスト（音声・ネット不要） |
| アダプタ層 | `#[ignore]` + `harvest` feature の統合テスト。エンジン稼働時のみ手動実行 |
| e2e | Step 1 の手動手順をスモークスクリプトとして固める |
| 既存コア | **触らない**（無改造の証明として、既存テストが素通りすること） |

## 9. 横断リスク表

| リスク | 兆候 | 手当て |
| --- | --- | --- |
| TTS→人間の転移性が低い（Q3） | evaluate は下がるが体感が変わらない | v0.1 の手動 repl サンプルと定期突合。Step 0 で最初に測るのはこのため |
| mlx-audio の破壊的変更 | CLI / 関数名が変わる | バージョン固定（`uv pip install 'mlx-audio==X.Y.Z'`）。アダプタ 1 ファイルに隔離済み |
| Zenn 非公式 API の変更 | 4xx・レスポンス形状の変化 | キャッシュで当面継続 → RSS フォールバック → Qiita 単独でも成立する設計 |
| Qiita レート制限 | 403 / 429 | `QIITA_TOKEN` 設定・キャッシュ・`--count` を抑える |
| exFAT ボリューム上の cargo / git の mtime 取りこぼし | 変更が拾われない | `touch` で対処（既知の罠）。キャッシュ I/O が遅ければ `--cache-dir` を内蔵ディスクへ |
| VOICEVOX 不在の環境 | 接続拒否 | `say` フォールバックで縮退動作 |

## 10. 決定・未決の台帳

決定 D1〜D6 と未決 Q1〜Q4 は SPEC（16・17 章）を正とする。本計画書は Step の
出口で Q を閉じる対応を持つ:

- Step 0 → Q3 の初測
- Step 1 → Q2（0.6B vs 1.7B）
- Step 4 の事前タスク → Q1（biasing 書式）
- Q4（UniDic）は除外ログの観察で随時判断

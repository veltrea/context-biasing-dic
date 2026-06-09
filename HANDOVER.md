# biasdiff harvest/evaluate 実装ハンドオーバー

前セッション（2026-06-10）で完成した仕様設計書と実装計画書をベースに実装を開始するための引き継ぎ文書。

## 前セッションの成果物

| ファイル | 内容 | 読むべき順 |
|---|---|---|
| [SPEC.md](SPEC.md) | 仕様設計書（英語・メイン） | 1 番目 |
| [SPEC.ja.md](SPEC.ja.md) | 仕様設計書（日本語） | 1 番目と並行 |
| [harvest-implementation-plan.md](harvest-implementation-plan.md) | 実装計画書（日本語・Step 別） | 2 番目。本格実装前に必読 |

メモリ記録: `~/.claude/projects/<this-project>/memory/biasdiff-harvest-direction.md` に経緯（API 検証・ASR の biasing 確認・決定 D1–D6）が記録されている。

## これからやることの要点

biasdiff v0.2 = `harvest` + `evaluate` サブコマンドの追加。v0.1 の「手動で文を読み上げる」ループを自動化する。

核心: **TTS に入れたテキストは定義上の正解として diff に流す** → モデル不要・完全ローカル。

### 全体フロー（図は SPEC.md § 3 参照）

```
fetch（Qiita/Zenn/local）→ extract（文抽出・フィルタ）→ TTS（VOICEVOX/say）
  → ASR（Qwen3-ASR MLX）→ 既存 diff コア（無改造）→ 辞書出力

evaluate: 辞書を biasing 投入して再認識 → 衝突率の頭打ちで語数確定
```

## これから何をするか：Step 0 から開始

### Step 0: 配管検証（コードを書く前に）

**目的**: (a) ツール鎖が通ることの確認。(b) SPEC Q3（TTS 音声から同音衝突が出るか）の最初の実測。

**実装計画書** § 2 の手順をそのまま実行：

```sh
# 1. mlx-audio を導入（uv 仮想環境）
uv venv ~/.venvs/mlx-audio
source ~/.venvs/mlx-audio/bin/activate
uv pip install mlx-audio
python -m mlx_audio.stt.generate --help

# 2. VOICEVOX で文を合成
TEXT="機械学習で意思決定を支援する仕組みを実装する"
# （エンコード → /audio_query → /synthesis → WAV）

# 3. ffmpeg で 16kHz mono WAV へ正規化
ffmpeg -y -i sent.wav -ar 16000 -ac 1 sent16k.wav

# 4. Qwen3-ASR で認識
python -m mlx_audio.stt.generate \
  --model mlx-community/Qwen3-ASR-0.6B-8bit --audio sent16k.wav

# 5. 既存 repl に貼って diff
cargo run --release -- repl
```

**完了条件**: 鎖が通り、`[+]` （同音衝突）が観測できれば成功。記録すること。

### Step 1 以降の大まかな作業

実装計画書 § 3–7 参照。コミット粒度は「各 Step ごと 1 コミット」で、出口の「動く縦穴」ごとに打ち止めにする（設計書 10 章の流儀）。

| Step | 内容 | 新規モジュール |
|---|---|---|
| 1 | トレイト3種 + 最小 harvest（FileSource） | `source.rs` / `synth.rs` / `recognize.rs` / `vote.rs` / `harvest.rs` + アダプタ 4 つ |
| 2 | `extract` 実装 + Qiita/Zenn ソース | `extract.rs` / `source_qiita.rs` / `source_zenn.rs` |
| 3 | 多声マトリクス + 投票 | `vote.rs` 実装完成 |
| 4 | `evaluate` | `evaluate.rs` |
| 5 | 夜間自動回し | `scripts/nightly-harvest.sh` |

## キー決定事項（SPEC § 16）

- **D1** 完全ローカル（VOICEVOX / say / ffmpeg / Qwen3-ASR on MLX）
- **D2** Rust 一枚岩。エンジンは subprocess / local HTTP。`default` feature には何も足さない
- **D3** ASR = Qwen3-ASR 0.6B-8bit（mlx-audio）。biasing は `system_prompt` で投入（ソース検証済み）
- **D4** 初期ソース = Qiita（公式 API）+ Zenn（非公式・ポライトネスモード）+ FileSource
- **D5** `extract` は純粋・std のみ。テストファースト
- **D6** `evaluate` は既存分類器の衝突率を指標に使う

## 未決事項（SPEC § 17）

| Q # | 内容 | 解決予定 |
|---|---|---|
| Q1 | biasing prompt 書式（区切り・前置き） | Step 4 の事前タスク |
| Q2 | 0.6B vs 1.7B の精度/速度 | Step 1（Step 0 で先行観察） |
| Q3 | TTS 衝突が人間発話に転移するか | **Step 0 で最初に測る**（賭けの検証） |
| Q4 | UniDic に切り替えるタイミング | 実行中に除外ログを観察して判断 |

## 実装に入る前に

### 読むべき既存コード
- `src/lib.rs` — コア層の構成（`token` / `diff` / `pipeline` / `collect`）
- `src/cli.rs` — CLI の arg パースと flow（`batch` / `repl` は実装が完了）
- `src/pipeline.rs` — `process()` 関数（v0.2 で reuse される中心）
- `app/src-tauri/src/lib.rs` — GUI バックエンドが既にコアを使っている（参考）

### テスト戦略（実装計画書 § 8）
- 純粋層（`extract` / `vote`）→ `cargo test`（網羅的）
- オーケストレーション → トレイトモック（ネット・音声不要）
- アダプタ → `#[ignore]` + `harvest` feature（手動実行）
- **既存コア → 触らない**（無改造が証明）

### git 運用上の注意
- `.gitignore` に `harvest_cache/` を追加（記事本文・音声を含むため）
- コミットメッセージに AI の Co-Authored-By を入れない（CLAUDE.md § 禁止）

## 環境確認（2026-06-10 時点）

```
✅ VOICEVOX 0.25.1 起動中（127.0.0.1:50021）
✅ macOS say 日本語 9 話者
✅ ffmpeg
✅ Rust toolchain + biasdiff v0.1 ビルド済み
❌ mlx-audio → Step 0 で導入（uv）
```

リモート Mac mini M4 でも同じ環境を整える必要が出たら、Step 5 の時点で。
実装は開発機で完結させる。

## 開始時の最初の作業

1. SPEC.md / SPEC.ja.md を通読（§1–4）
2. harvest-implementation-plan.md を読む（§0–3）
3. 実装計画書 § 2 の Step 0 コマンド列をそのまま実行
4. 鎖が通ることを確認 + Q3 を初測
5. 本格実装は Step 1 から（§3 参照）

## Questions / Decisions Log

このセッション中に新しく決まったこと / 質問が出たことは、本ファイル下部に追記する。最後に memory に反映する。

### 本セッション中の追記欄

#### Step 0 完了（2026-06-10）— 配管検証 + Q3 初測

**結論: 鎖全体が通り、`[+]` 同音衝突を観測。Q3 に強い好材料。**

**導入したバージョン（記録）**:
- mlx-audio **0.4.4**（`uv venv ~/.venvs/mlx-audio`、CPython 3.12.11）
- VOICEVOX 0.25.1 / 話者 3、ffmpeg、Qwen3-ASR 0.6B-8bit & 1.7B-8bit（HF キャッシュ済み）

**CLI 表記の確定（計画書 § 2 のリスク欄への回答）**:
- `python -m mlx_audio.stt.generate --model mlx-community/Qwen3-ASR-0.6B-8bit --audio in.wav --output-path out`
- **`--output-path` は必須引数**（計画書のコマンド例には無かった）。出力は `{output-path}.txt`、文末に「。」付き
- **`--context` オプションが CLI に存在**（"Context string with hotwords"）→ Q1 の biasing は
  `system_prompt` を自前で組まず `--context` で渡せる見込み。Step 4 前に実装を読んで確定
- `--language ja` も指定可

**所要時間（開発機実測）**:
- TTS（VOICEVOX）+ ffmpeg 16kHz 正規化: 約 1.1 秒/文
- ASR 0.6B: 約 2.1〜2.3 秒/文、1.7B: 約 2.7 秒/文 — **どちらもプロセス起動・モデルロードが支配的**
  → SPEC § 9 のバッチドライバ（モデル 1 回ロード）の価値を実測で確認
- 初回モデル取得: 0.6B 約 15 秒 / 1.7B 約 30 秒。`cargo build --release`: 52 秒

**Q3 初測（同音異義語を仕込んだ 10 文、話者 3・等速）**:
- 0.6B: **危険語 8 語**（仕様←使用・用件←要件・動機←同期・意向←以降・課程←過程・主導←手動・対照←対象・対称←対象）、除外 5 件
- 1.7B: **危険語 10 語**（上記 ± : +確率←確立・+解答←回答・+データ移行(偽陽性・下記)・−意向・−回答）、除外 3 件
- 除外側のフェイルセーフも設計どおり（機械/非会 = キカイ/ヒカイ の読み不一致は正しく落ちた）

**Q2 事前観測**: 1.7B が一様に良いわけではない（文 5 は 0.6B より派手に崩壊:
「データ移行は以降の…意向だ」→「低代号は代号の…代号だ」）。速度差も小。
収穫器としては「適度に間違える」0.6B 既定の方針を支持。本計測は Step 1 で。

**新規の学び（要対応の観察）**:
1. **loose 正規化の濁点畳み込みが偽陽性を通し得る**: 「データ移行(テ゛ータイコー)←低代号(テイダイゴー)」が
   清音化後どちらも「テータイコー」になり `[+]` 採用された（`--strict` なら除外）。
   ジッソウ≈シッソウ を拾うための意図された設計の裏面。**多声投票（Step 3）の必要性を裏付ける実例**として記録。
2. ASR は読点「、」を挿入する癖がある。文 7（0.6B）で「回答」が「、解答」とアラインされ
   読み不一致扱いに（収穫漏れ・フェイルセーフ方向）。Step 1 で hypothesis 側の句読点正規化を検討する価値あり。
3. 「機械→非会」は両モデル共通の誤り（話者 3 の音響に起因の可能性）。別話者で消えるなら多声投票の好例。

**検証用作業ファイル**: `/tmp/biasdiff-step0/`（sentences.txt / hypothesis*.txt / reject*.txt。/tmp なので揮発）

#### Step 1 完了（2026-06-10）— トレイト 3 種 + 最小 harvest（FileSource）

**完了条件を満たした**: `biasdiff harvest --source file --input sentences.txt --tts voicevox --asr qwen3-mlx` の
1 コマンドで file → TTS → ASR → 既存コア（無改造）→ amical-json が通り、再実行が冪等。

**実測（10 文・VOICEVOX 話者 3・0.6B）**:
- 一周目 29.2 秒（cold）→ **二周目 0.025 秒**（キャッシュ全命中 audio 10/10・asr 10/10、出力完全一致）
- 危険語 **9 語**: Step 0 の手動 batch（8 語）より 1 語多い。差分は「回答←解答」—
  diff 前の**全角句読点除去**（harvest.rs の `strip_punct`、Step 0 の学び 2 への対処）で
  ASR の挿入読点によるアライン崩れが消えたため。除外 4 件（機械/非会ほか、フェイルセーフ動作）

**Q2 本計測（同 10 文を両モデルで）**:
- 0.6B: 危険語 9、ASR 約 2.2 秒/文 ／ 1.7B: 危険語 10（**偽陽性「データ移行←低代号」込み**）、約 2.3 秒/文
- どちらもモデルロード支配で速度差は誤差。1.7B が一様に良いわけではない（文 5 は 1.7B の方が派手に崩壊）
- → **既定 0.6B 維持で確定**。バッチドライバ（SPEC §9、モデル 1 回ロード）は今後も価値あり

**実装の設計判断（SPEC へ反映済み・英日同期)**:
- `Synthesizer::synth(text, voice, out: &Path)` — 出力先（内容アドレスのキャッシュ位置）は
  オーケストレータが決めて渡す。アダプタにキャッシュの知識を持ち込まない（SPEC §5 更新）
- ASR キャッシュキー = `sha256(audio-key | model)` — モデル切り替えで古い結果を流用しない
  （SPEC §12 更新。Q2 計測がこの設計の実地検証になった: 1.7B 実行時 audio 10/10 命中・asr 0/10）
- キャッシュ書き込みは一時ファイル（`.part`）+ rename。中断の半端ファイルを命中と誤認しない
- 投票（vote.rs）は Step 1 から実装済み（min_votes=1 でパススルー）。NonHomophone は投票対象外で素通し

**実環境で見つけたバグ（モックでは不可視・記録に値する）**:
- ffmpeg は**出力ファイルの拡張子**からフォーマットを推定するため、`.part` 一時パスへの出力が
  「Unable to choose an output format」で全滅した。`-f wav` 明示で解決（ffmpeg.rs）。
  教訓: subprocess 系アダプタの統合は実エンジンで一周してから信用する

**テスト**: default 28 / harvest 35 全通過（既存コアのテストは素通り = 無改造の証明）。
`#[ignore]` 統合テスト 2 本も実エンジン（VOICEVOX + venv）で通過。実行方法:
`source ~/.venvs/mlx-audio/bin/activate && cargo test --features harvest -- --ignored`

**使い方メモ**: venv を activate してから実行すれば `--asr-python` 省略可（既定 python3 が venv を指す）。
dry-run は VOICEVOX 不在でも動く（疎通確認をスキップ）。`--min-votes` 省略時は話者 2 以上の構成で 2、
それ以外 1（SPEC §11 の既定）。

**次は Step 2**: `extract`（テストファースト・例文化の品質 = 辞書の純度）+ QiitaSource / ZennSource +
`articles/{source}/{id}.json` / `seen.jsonl` キャッシュ + `--dry-run` の本領発揮。

---

**作成**: 2026-06-10（前セッション）
**更新**: （このセッションで適宜）

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

#### Step 2 完了（2026-06-10）— extract + QiitaSource / ZennSource

**完了条件を満たした**: `--source qiita` / `--source zenn` の dry-run が実記事で動き、
例文は目視でほぼ全数「読み上げて自然」（完了条件の 8 割を大きく超過）。

**extract.rs（純粋・std のみ・正規表現不使用・テスト 22 本）**:
- Markdown: フェンス/表/見出し/引用/リスト記号の行処理 + リンク [text](url)→text・
  画像→除去・URL 除去・バッククォート/アスタリスク剥がし
- HTML: `<script>`/`<style>`/`<pre>` は中身ごと、他はタグ剥がし、エンティティ最低限デコード
- フィルタ: 20〜80 字・日本語率 ≥0.5・残骸文字（| ` < > { } \ = # $ ; _ ~ [ ] と ".."・"http"）・
  **括弧の開閉不一致**（文断片の検出）。スコアは漢字率降順・記事内上限 20 文・正規化重複排除

**実記事の目視で見つけてフィクスチャに還元したゴミ 4 種**（テストファーストの実践）:
1. 裸ブラケット「[追記] …」→ 残骸文字に `[` `]` 追加
2. 閉じない括弧「…「なにそれ面白そう」（文分割の断片）→ 括弧バランスチェック追加
3. 連続ピリオド「…感じます..」→ ".." を残骸に追加
4. **Zenn はインラインコードも `<code>` タグ** → 中身ごと消すと「 や  によって」の穴あき文になる。
   `<code>` はタグだけ剥がし中身を残す（Markdown のバッククォート扱いと対称、ブロックは `<pre>` が消す）

**seen.jsonl（実行間の増分管理）の設計判断**:
- `TextSource::dedup_across_runs()` — qiita/zenn は true、**file は false**（明示入力は毎回フル処理 =
  同じ入力から同じ辞書が再現できる。Step 1 の冪等性テストと両立させるため）
- 記録は「処理が失敗なく完了した単位」のみ（文・記事とも）。失敗した文は次回再処理され、
  高価な部分はキャッシュが守る。壊れた行は黙って読み飛ばす（再処理方向に倒れる）
- 文キー = `sha256(空白除去後の文)`。std の DefaultHasher は版間の安定保証がないので不使用

**ソースアダプタ**:
- Qiita: 公式 API v2、一覧 1 リクエストに本文（Markdown）同梱、`QIITA_TOKEN` 対応（env から）。
  記事 JSON は `articles/qiita/{id}.json` に保存（参照・監査用。一覧は鮮度優先で毎回叩く）
- Zenn: 一覧（メタ）→ 詳細（`body_html`）の 2 段。**1 req/s スロットル実測ログ:
  [1.0s] → [2.2s] → [3.3s] → [4.4s]**（毎リクエスト前 1 秒 sleep）。ツール名 UA。
  詳細は `articles/zenn/{slug}.json` キャッシュ命中でリクエストゼロ（2 回目 dry-run で確認）。
  1 記事の取得失敗は warn して続行

**テスト**: default 50 / harvest 60（53+α）→ 全通過。既存コアは素通り（無改造の証明継続）。

**次は Step 3**: 多声マトリクス + 投票の実測（投票が偽陽性を削る実例の記録）。
vote.rs と CLI の `--voices`/`--rates`/`--min-votes` は実装済みなので、
残りは「データ移行←低代号」級の偽陽性が実際に削れることの実証実験が主。

#### Step 3 完了（2026-06-10）— 多声マトリクス + 投票の実証

**完了条件を満たした**: 投票が話者固有の偽陽性を削った実例を記録。

**実証実験（10 文 × 3 話者: ずんだもん 3・四国めたん 2・玄野武宏 11、0.6B）**:
```
vote: 9 pair(s) adopted, 1 dropped (fewer than 2 distinct speakers):
  [voted out] データ移行 ← データイコウ (1 speaker)
```
- ある話者でだけ ASR が「データ移行」を**カタカナ表記**「データイコウ」で書き起こし、
  読み一致（当然）で採用候補化 → **1 話者のみの観測なので投票が棄却**。
  Step 0 の懸念（loose 正規化を通る表記ゆれ・話者固有の偽陽性）を投票が実際に防いだ
- 採用 9 語の票数: 3 票（全話者で衝突 = 頑健）が主導・仕様・動機・対照・対称・用件・課程の 7 語、
  2 票が回答・意向。頻度 = 観測数の意味論も維持（SPEC §11 どおり）
- `--min-votes 1` で「データ移行 1」が末尾に復活（従来挙動へ戻る）。全 30 セルキャッシュ命中で即時

**Step 3 で発見・修正した退行（重要）**:
- 3 話者実験の 1 回目で「10 文中 5 文しか処理されない」ことを発見。Step 2 の extract 統合で
  **FileSource の明示入力（1 行 1 文）にも 20 字下限フィルタがかかり**、19 字以下の 5 文が
  黙って捨てられていた（Step 1 の不変条件の退行）
- 修正: `TextSource::trusted_input()`（既定 false）を追加。FileSource は true →
  `ExtractOptions::lenient()`（構造除去・文分割・重複排除のみ、品質フィルタなし）で抽出。
  記事ソース（qiita/zenn）は従来どおり品質ゲートあり
- あわせて extract のスコア選抜を「上限超過時のみソート → 選抜後は出現順に復元」へ変更
  （明示入力の順序維持・dry-run が記事の流れどおりに読める）
- 投票棄却の内訳を CLI の verbose に追加（`[voted out] 表記 ← 表記 (話者数)`）—
  偽陽性が削れたことを目で確認できる肝

**テスト**: default 51 / harvest 61 全通過。

**次は Step 4**: `evaluate`。事前タスクとして Q1（biasing prompt 書式）を閉じる —
公式 `QwenLM/Qwen3-ASR` の `context=` 実装を読み、`--context` への渡し方（区切り・前置き）を確定。
mlx-audio 0.4.4 の CLI に `--context` があることは Step 0 で確認済み。

#### Step 4 完了（2026-06-10）— evaluate + Q1 解決 + バッチドライバ

**Q1 解決（Step 0 の見込みを覆す重要発見）**: mlx-audio 0.4.4 の CLI は `--context` を受けるが、
kwargs を `inspect.signature(model.generate)` で濾すため **Qwen3 の generate に無い `context` は
黙って捨てられる**（`--prompt` も同様。導入済みソース精読で確認）。biasing が届く唯一の経路は
Python API の `system_prompt=`。書式は半角スペース区切り・前置きなし（公式 `context` 例と同形）。
→ 教訓「README・--help でなく実装本体を読む」の再演。

**biasing の実機実証（プロジェクト価値仮説の初検証）**:
- bias なし:「**非会**学習で意思決定を…」（Step 0 から再現する誤り）
- bias = 関連 3 語:「**機械**学習で…」に修正
- **bias = 収穫済み辞書 10 語でも修正** — biasdiff が集めた語がそのまま biasing として機能

**バッチドライバ（scripts/qwen3_asr_batch.py + アダプタ全面書き換え）**:
- SPEC §9 の JSONL 契約 + ready ハンドシェイク。`include_str!` で埋め込み・実行時 temp 実体化
  （バイナリ自己完結）。`RefCell<Option<Child>>` の遅延起動・Drop で stdin close → wait
- **10 文 21 秒 → 4.3 秒**（モデル 1 回ロード、約 5 倍速。1 文 ~0.2 秒）

**evaluate 実装**:
- 入力は harvest が残す **refs.jsonl**（音声キー → 正解文。SPEC §12 に追記）。既存キャッシュからは
  harvest 再実行（全命中・一瞬）で再構築できることを確認
- N スケジュール 0..step..max（辞書サイズで終端）、`asr-biased/` キャッシュは
  **キーに bias 語リスト内容を含む**（辞書が変われば別キャッシュ。SPEC §12 更新）。N=0 は
  harvest の asr/ キャッシュとキー互換（収穫済みならゼロコスト）
- 頭打ち: `--min-delta`（既定 0.01）/`--patience`（既定 2）。`--report` curve.tsv、
  `--prune` = N=0 で衝突 ∧ 最終 N で消えた語（実際に直した語）
- **CLI は `--input`**（`--dict` はグローバルの形態素辞書切替と clap 名前衝突 — 実行時 panic で発覚。
  SPEC §13 更新）

**実走（10 文 × 3 話者 = 30 音声、9 語辞書、N=0/3/6/9）**: 初回 26.7 秒・再実行 0.025 秒（冪等）
- curve: N=0: 26 衝突 → N=3: 25 → N=6: 25 → **N=9: 27（悪化）**。推奨 N=3、prune = 主導・仕様・動機
- **N=9 の悪化は SPEC リスク欄「語数を絞るほど効く帯」の実観測**。同音衝突を意図的に詰め込んだ
  文セットでは bias 語同士が干渉する（対照・対称を同時投入すると全部そちらへ倒れる等）。
  実記事由来の自然なセットでの再測定は Step 5 以降の宿題

**テスト**: default 51 / harvest 65 全通過（evaluate 4 本追加: カーブ・辞書内容別キャッシュ・
refs 無しエラー・plateau エッジ）。

**次は Step 5**: 夜間自動回し（scripts/nightly-harvest.sh、WoL → SSH → 取り込み → shutdown、
マシン固有情報はリポジトリ外）。GUI 統合は任意項目。

#### Step 5 完了（2026-06-10）— 夜間自動回し（ローカル検証まで）

**scripts/nightly-harvest.sh**: 全パラメータを `BIASDIFF_*` 環境変数で注入する汎用スクリプト
（リポジトリにマシン固有情報なし）。`BIASDIFF_REMOTE_HOST` 未設定なら**ローカル実行**、
設定時は WoL（`BIASDIFF_REMOTE_MAC`）→ SSH 疎通待ち（最大 3 分）→ リモートで自分自身を
ローカルモード再帰実行 → `BIASDIFF_FETCH_TO` へ scp 取り込み → `BIASDIFF_SHUTDOWN=1` で
shutdown（パスワードレス sudo 前提）。流れ: qiita + zenn 収穫 → 当日 counts を awk マージ →
evaluate → `nightly/{日付}/`（dict.counts.tsv / curve.tsv / pruned.txt / 各ログ）。
片ソース失敗は警告で続行（API 変更・レート制限で夜間全体を殺さない）。`nightly/` は git-ignore。

**手動 1 回流し（ローカル、実記事 qiita 2 + zenn 2、話者 3 単独、4.5 分）**:
- マージ辞書 23 語、評価 80 音声・320 認識・失敗 0
- **実記事由来の自然セットでは衝突率カーブが単調減**: N=0: 0.325 → N=20: 0.200。
  Step 4 の極端セットで見えた「N 増で悪化」は出ず、**現実条件で辞書が素直に効く**ことを確認
- 「still improving at max-words」の診断どおり、23 語では頭打ちに達しない = 収穫を続けるほど
  辞書が育つ余地がある（夜間回しの存在意義そのもの）
- pruned には 型・WebView・スタック・テスタビリティ等の技術語が並ぶ（実際に衝突を直した語）。
  一方 dict には「いえ・とき・ほう」級の一般語ノイズも残る — 多声投票（3 話者）と
  頻度の積み上げで自然に沈む見込み。気になるなら将来「最低頻度」フィルタを検討

**リモート運用（Mac mini M4）を始めるときの手順（未実施・次の宿題）**:
1. Mac mini に VOICEVOX・`uv venv ~/.venvs/mlx-audio` + `uv pip install mlx-audio==0.4.4`・
   リポジトリ clone・`cargo build --release --features harvest` を整える
2. パスワードレス sudo（shutdown 用）を設定
3. 開発機の launchd に毎晩のジョブを登録。例（`~/Library/LaunchAgents/com.biasdiff.nightly.plist`）:
   ProgramArguments = [bash, -lc, "BIASDIFF_REMOTE_HOST='<user>@<mac-mini-ip>'
   BIASDIFF_REMOTE_DIR='~/dev/context-biasing-dic' BIASDIFF_REMOTE_MAC='<mac-address>'
   BIASDIFF_SSH_OPTS='-o ProxyJump=none -o IdentitiesOnly=yes -i ~/.ssh/id_ed25519'
   BIASDIFF_FETCH_TO=$HOME/biasdiff-nightly BIASDIFF_SHUTDOWN=1
   /path/to/repo/scripts/nightly-harvest.sh"]、StartCalendarInterval = {Hour: 3}
   （**192.168.1.x への SSH は ProxyJump=none 必須** — CLAUDE.md の罠メモ参照）

**v0.2 ロードマップ（Step 0〜5）はこれで完走。** 残る運用上の宿題: リモート常設、
人間発話との突合（Q3 の継続検証、v0.1 repl と定期比較）、UniDic 切替の判断（Q4、除外ログ観察）。

#### リモート常設 完了（2026-06-10）— Mac mini M4 で夜間運用開始

**整えたもの（Mac mini 側、ユーザー veltrea）**:
- brew で uv・ffmpeg 導入（brew update の競合ロックに一度はまった → ロック掃除 + `brew update` で解決）
- `uv venv ~/.venvs/mlx-audio` + `mlx-audio==0.4.4`（バージョン固定）、Qwen3 0.6B は warmup 済み（HF キャッシュ）
- リポジトリ clone（`~/dev/context-biasing-dic`・public なので認証不要）+ `cargo build --release --features harvest`（49 秒。Rust は導入済みだった）
- **VOICEVOX はアプリ同梱エンジンの headless 起動**: `/Applications/VOICEVOX.app/Contents/Resources/vv-engine/run --host 127.0.0.1 --port 50021`（エンジン 0.24.1・話者 3/2/11 確認）。
  LaunchDaemon `/Library/LaunchDaemons/local.voicevox-engine.plist`（UserName 指定・RunAtLoad・KeepAlive）
  で**ブート時にログイン不要で常駐**
- passwordless sudo: `/etc/sudoers.d/biasdiff-shutdown`（`/sbin/shutdown` のみ NOPASSWD。visudo -c 検証済み）

**開発機側**: `~/Library/LaunchAgents/com.biasdiff.nightly.plist`（**毎晩 3:00**、
EnvironmentVariables で REMOTE_HOST/DIR/MAC・SSH_OPTS・FETCH_TO=`~/biasdiff-nightly`・SHUTDOWN=1 を注入、
ログ `/tmp/biasdiff-nightly.log`）。実マシン値は plist にのみ存在（リポジトリには載せない）。

**リモート検証実走（COUNT=1・2 話者）**: 辞書 11 語、curve N=0: 0.425 → N=10: 0.363、ASR 失敗 0、
成果物 fetch 成功。Mac mini は shutdown 済みで、**今夜 3:00 が WoL → 収穫 → 取り込み → shutdown の
フルサイクル初実走**。朝の確認: `~/biasdiff-nightly/{日付}/` と `/tmp/biasdiff-nightly.log`。

**実走で見つけて直した 2 件（コミット済み）**:
1. SSH non-login shell に Homebrew PATH が無く ffmpeg 不可視 → スクリプト冒頭で PATH 自衛（e96d332）
2. `BIASDIFF_VENV` の既定値がローカルの `$HOME` で展開されてリモートへ漏れ、存在しない venv を探して
   ASR 全滅 → **明示時のみ転送**に修正（25e6cae）。このとき「失敗した文は seen に記録しない」設計が
   実地で機能: 事故回の記事は seen に入らず、修正後の再実行で音声キャッシュを再利用しつつ正しく収穫された

#### 構成変更（2026-06-10 同日）— Mac mini 完全自律型へ

ユーザー指摘「スケジュールはなぜローカル（開発機）？ テスト環境は Mac mini では」が正しく、
調査で旧構成（開発機司令塔 + shutdown + WoL）の問題が連鎖的に発覚したため、同日中に組み替えた。

**発覚した問題（いずれも今夜の初回実行を壊していた）**:
1. **Apple Silicon Mac は shutdown 状態から WoL で起こせない**（スリープからのみ）。
   午前の「WoL 成功」は既に起動していたマシンへの SSH（`up after ~0s`）で、未実証だった
2. ブート直後は VOICEVOX エンジンの LaunchDaemon 初期化が間に合わず、収穫が即死するレース
   → スクリプトに**エンジン起動待ち（最大 2 分）**を追加（b612f71）
3. `shutdown -h now` が **GUI ログイン中ユーザー（LM Studio 用アカウント）にブロックされて未完了のまま固まり**、
   nologin ファイル残留で「ping・画面共有は生きるが SSH だけ NO LOGINS 拒否」という紛らわしい状態に。
   復旧は GUI からの再起動（ユーザー実施）
4. `pmset sleepnow` は **root 必須**（一般ユーザー可は誤った前提）→ sudoers を
   `/sbin/shutdown, /usr/bin/pmset sleepnow` の 2 コマンド NOPASSWD に拡張、スクリプトは `sudo -n`（06fe014）

**新構成（時刻管理を Mac mini 自身に移譲・開発機は不要）**:
- mini: `pmset repeat wakeorpoweron MTWRFSU 02:55:00`（自己起床。スリープからは wake、電源断からは power on）
- mini: LaunchDaemon `local.biasdiff-nightly`（3:00、ローカルモード、`BIASDIFF_SHUTDOWN=sleep`、
  HOME/BIASDIFF_VENV を plist で明示）→ 収穫後は**スリープ**（shutdown ではなく）
- スリープ運用の利点: 日中も WoL で数秒で起こせる（消費 1W 未満）+ GUI セッションブロック問題と無縁
- 開発機: 旧 launchd ジョブ `com.biasdiff.nightly` を撤去。朝 8:00 の確認タスクは
  「WoL で起こす → mini 上のログ・成果物（`~/dev/context-biasing-dic/nightly/{date}/`）を確認 →
  スリープに戻す」の新構成版へ更新

**実証済み（pmset 電源ログで裏取り）**:
- VOICEVOX デーモンのブート自動起動（ユーザーの再起動後に 0.24.1 即応答）
- `env -i` の最小環境（LaunchDaemon 近似）で nightly が完走（seen による増分スキップも正常動作）
- 強制スリープ投入（`Entering Sleep ... 'Software Sleep'`）→ **WoL 復帰（`DarkWake due to Enet.MagicPacket`、
  SSH 復帰まで約 3 秒）**のフルサイクル

**今夜 3:00 が自律サイクルの初実走**（2:55 自己起床 → 収穫 → スリープ）。成果物は mini 側に蓄積。

---

**作成**: 2026-06-10（前セッション）
**更新**: （このセッションで適宜）

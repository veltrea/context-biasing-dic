//! `harvest` のオーケストレーション（SPEC 3・12 章）。
//!
//! fetch → 文化 → 声マトリクス × (合成 → 認識) → 既存 diff コア（無改造）→
//! 投票 → 集計、の一本道。エンジンへはトレイト（`TextSource` / `Synthesizer` /
//! `Recognizer` / `Tokenizer`）越しにのみ触れるため、ここはモックで単体テスト
//! できる。
//!
//! 高価な生成物（音声・認識結果）は内容アドレスでキャッシュする。中断して
//! 再実行しても支払い済みの仕事は繰り返さない（冪等性）。書き込みは
//! 一時ファイル + rename で行い、中断による半端なファイルをキャッシュ命中と
//! 誤認しない。

use crate::collect::Collector;
use crate::extract::{self, ExtractOptions};
use crate::pipeline::process;
use crate::reading::NormalizeOptions;
use crate::recognize::Recognizer;
use crate::source::TextSource;
use crate::synth::{Synthesizer, VoiceSpec};
use crate::token::Tokenizer;
use crate::vote::{VoteBook, VoteSummary};
use anyhow::{Context, Result};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;

/// オーケストレーションが依存する 4 つの抽象。すべて注入（テストはモック）。
pub struct HarvestDeps<'a> {
    pub source: &'a dyn TextSource,
    pub synthesizer: &'a dyn Synthesizer,
    pub recognizer: &'a dyn Recognizer,
    pub tokenizer: &'a dyn Tokenizer,
}

/// 実行オプション。CLI 引数から組み立てる。
pub struct HarvestOpts {
    /// ソースから取得する記事数の上限。
    pub count: usize,
    /// 声マトリクス（話者 × 話速の展開済みリスト）。
    pub voices: Vec<VoiceSpec>,
    /// 採用に必要な異なり話者数（SPEC 11 章）。
    pub min_votes: usize,
    /// キャッシュディレクトリ（`audio/`・`asr/` を下に作る）。
    pub cache_dir: PathBuf,
    /// 読み比較の正規化（v0.1 と同じ意味）。
    pub normalize: NormalizeOptions,
    /// ASR モデル名。認識結果キャッシュのキーに入る（モデル違いは別結果）。
    pub asr_model: String,
    /// 例文化で止めて文リストを返す（TTS/ASR を回さない）。
    pub dry_run: bool,
    /// 進捗を stderr に出す。テストでは false。
    pub verbose: bool,
}

/// 実行結果。表示・書き出しは CLI の責務（ここでは集計だけ持つ）。
pub struct HarvestReport {
    pub collector: Collector,
    pub vote: VoteSummary,
    /// 例文化後の文数（seen によるスキップ適用後）。
    pub sentences: usize,
    /// 処理セル数（文 × 声）。
    pub cells: usize,
    pub audio_cache_hits: usize,
    pub asr_cache_hits: usize,
    /// 合成・認識に失敗して飛ばしたセル数（失敗は run 全体を止めない）。
    pub failures: usize,
    /// seen.jsonl により処理を飛ばした記事数（実行間の重複排除）。
    pub skipped_articles: usize,
    /// seen.jsonl・実行内重複により飛ばした文数。
    pub skipped_sentences: usize,
    /// dry-run のときだけ Some。例文化の結果。
    pub dry_run_sentences: Option<Vec<String>>,
}

/// 例文化済みの 1 記事分。記事の全文が処理し終わった時点で seen に記録する
/// ため、記事と文の対応を保ったまま処理ループへ渡す。
struct PendingArticle {
    /// seen.jsonl のキー（`{label}/{id}`）。
    key: String,
    sentences: Vec<String>,
}

/// harvest 一周。
pub fn run(deps: &HarvestDeps, opts: &HarvestOpts) -> Result<HarvestReport> {
    let articles = deps.source.fetch(opts.count)?;
    let dedup = deps.source.dedup_across_runs();
    let label = deps.source.label();

    // seen は dry-run でも読む（「実際に処理される文」を見せる）が、書くのは
    // 実処理が完了した単位だけ。ファイルが無ければ空集合になるだけで、
    // ここではキャッシュディレクトリを作らない。
    let mut seen = Seen::load(&opts.cache_dir);

    // 例文化（extract: 構造除去 → フィルタ → スコア → 記事内重複排除）に、
    // 実行内の記事間重複と、seen による実行間重複の排除を重ねる。
    // 明示入力（FileSource）は品質フィルタなしの lenient プロファイル。
    let extract_opts = if deps.source.trusted_input() {
        ExtractOptions::lenient()
    } else {
        ExtractOptions::default()
    };
    let mut pending: Vec<PendingArticle> = Vec::new();
    let mut in_run: BTreeSet<String> = BTreeSet::new();
    let mut skipped_articles = 0usize;
    let mut skipped_sentences = 0usize;
    for a in &articles {
        let akey = format!("{}/{}", label, a.id);
        if dedup && seen.has_article(&akey) {
            skipped_articles += 1;
            continue;
        }
        let mut kept: Vec<String> = Vec::new();
        for s in extract::extract(&a.body, &extract_opts) {
            let h = sentence_hash(&s);
            if (dedup && seen.has_sentence(&h)) || !in_run.insert(h) {
                skipped_sentences += 1;
                continue;
            }
            kept.push(s);
        }
        pending.push(PendingArticle {
            key: akey,
            sentences: kept,
        });
    }
    let total: usize = pending.iter().map(|p| p.sentences.len()).sum();

    if opts.dry_run {
        let sentences: Vec<String> = pending.into_iter().flat_map(|p| p.sentences).collect();
        return Ok(HarvestReport {
            collector: Collector::new(),
            vote: VoteSummary {
                passed_pairs: 0,
                dropped_pairs: 0,
                dropped: Vec::new(),
            },
            sentences: sentences.len(),
            cells: 0,
            audio_cache_hits: 0,
            asr_cache_hits: 0,
            failures: 0,
            skipped_articles,
            skipped_sentences,
            dry_run_sentences: Some(sentences),
        });
    }

    let cache = Cache::new(&opts.cache_dir)?;
    let mut book = VoteBook::new();
    let mut refs = Refs::load(&opts.cache_dir);
    let mut audio_cache_hits = 0usize;
    let mut asr_cache_hits = 0usize;
    let mut failures = 0usize;
    let mut done = 0usize;

    for article in &pending {
        // seen への記録は「処理が失敗なく完了した単位」だけ。中断・失敗した
        // 文/記事は次回も候補に残り、高価な部分はキャッシュが守る。
        let mut article_complete = true;
        for sent in &article.sentences {
            done += 1;
            let mut sentence_complete = true;
            for voice in &opts.voices {
                let started = Instant::now();
                // 音声: 内容アドレス（文 + エンジン + 話者 + 話速）。
                let audio_key =
                    content_key(&[sent, &voice.engine, &voice.voice, &voice.rate_key()]);
                let audio_path = cache.audio_path(&audio_key);
                let audio_was_cached = audio_path.exists();
                if !audio_was_cached {
                    let part = part_path(&audio_path);
                    if let Err(e) = deps.synthesizer.synth(sent, voice, &part) {
                        failures += 1;
                        sentence_complete = false;
                        let _ = fs::remove_file(&part);
                        eprintln!("warn: synth failed ({}): {e:#}", voice.speaker_key());
                        continue;
                    }
                    fs::rename(&part, &audio_path)
                        .with_context(|| format!("failed to move {} into cache", part.display()))?;
                }

                // 認識: 音声キー + モデル名（モデルが違えば別の結果）。
                let asr_key = content_key(&[&audio_key, &opts.asr_model]);
                let asr_path = cache.asr_path(&asr_key);
                let asr_was_cached = asr_path.exists();
                let hypothesis = if asr_was_cached {
                    fs::read_to_string(&asr_path)
                        .with_context(|| format!("failed to read {}", asr_path.display()))?
                } else {
                    match deps.recognizer.recognize(&audio_path, None) {
                        Ok(h) => {
                            let h = h.trim().to_string();
                            let part = part_path(&asr_path);
                            fs::write(&part, &h)
                                .with_context(|| format!("failed to write {}", part.display()))?;
                            fs::rename(&part, &asr_path)?;
                            h
                        }
                        Err(e) => {
                            failures += 1;
                            sentence_complete = false;
                            eprintln!("warn: asr failed ({}): {e:#}", audio_path.display());
                            continue;
                        }
                    }
                };
                if audio_was_cached {
                    audio_cache_hits += 1;
                }
                if asr_was_cached {
                    asr_cache_hits += 1;
                }
                // evaluate 用に「音声キー → 正解文」の対応を残す（SPEC 14 章の入力）。
                refs.record(&audio_key, sent)?;

                // 既存コアへ。ASR は句読点を即興で挿入する（Step 0 実測: 「回答」が
                // 「、解答」とアラインされ読み不一致扱いになった）ため、表記 diff の
                // 前に両側から句読点を除いてアラインの崩れを防ぐ。
                let outcomes = process(
                    deps.tokenizer,
                    &strip_punct(sent),
                    &strip_punct(&hypothesis),
                    &opts.normalize,
                )?;

                if opts.verbose {
                    let adopted = outcomes.iter().filter(|o| o.is_homophone()).count();
                    let rejected = outcomes.len() - adopted;
                    eprintln!(
                        "[{}/{}][{}@{}] audio={} asr={} {:.1}s [+]{} [-]{} | {}",
                        done,
                        total,
                        voice.speaker_key(),
                        voice.rate_key(),
                        if audio_was_cached { "cache" } else { "run" },
                        if asr_was_cached { "cache" } else { "run" },
                        started.elapsed().as_secs_f32(),
                        adopted,
                        rejected,
                        sent,
                    );
                }

                book.record(&voice.speaker_key(), outcomes);
            }
            if sentence_complete {
                if dedup {
                    seen.record_sentence(&sentence_hash(sent))?;
                }
            } else {
                article_complete = false;
            }
        }
        if article_complete && dedup {
            seen.record_article(&article.key)?;
        }
    }

    let mut collector = Collector::new();
    let vote = book.flush_into(&mut collector, opts.min_votes);

    Ok(HarvestReport {
        collector,
        vote,
        sentences: total,
        cells: total * opts.voices.len(),
        audio_cache_hits,
        asr_cache_hits,
        failures,
        skipped_articles,
        skipped_sentences,
        dry_run_sentences: None,
    })
}

/// seen.jsonl に永続する文キー: 正規化（空白除去）後の SHA-256。
/// std の `DefaultHasher` はバージョン間の安定保証がないため使わない。
fn sentence_hash(s: &str) -> String {
    content_key(&[&extract::normalize_sentence(s)])
}

/// `evaluate` が再利用する「音声キー → 正解文」の対応（`refs.jsonl`）。
///
/// 1 行 1 JSON（`{"audio":"<audio_key>","text":"<sentence>"}`、audio_key で
/// 一意）。文がキャッシュ内に残るのは `articles/` と同等の扱いで、
/// `harvest_cache/` ごと git-ignore 済み。evaluate はこれを読んで
/// 「キャッシュ済み音声セット + 正解」を組み立てる（SPEC 14 章の入力）。
pub(crate) struct Refs {
    path: PathBuf,
    known: BTreeSet<String>,
}

impl Refs {
    fn load(cache_dir: &Path) -> Self {
        let path = cache_dir.join("refs.jsonl");
        let mut known = BTreeSet::new();
        if let Ok(text) = fs::read_to_string(&path) {
            for line in text.lines() {
                let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else {
                    continue;
                };
                if let Some(a) = v.get("audio").and_then(|x| x.as_str()) {
                    known.insert(a.to_string());
                }
            }
        }
        Self { path, known }
    }

    /// 対応を 1 件記録する（audio_key が新出のときだけ追記）。
    fn record(&mut self, audio_key: &str, text: &str) -> Result<()> {
        if !self.known.insert(audio_key.to_string()) {
            return Ok(());
        }
        use std::io::Write as _;
        if let Some(dir) = self.path.parent() {
            fs::create_dir_all(dir)?;
        }
        let mut f = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
            .with_context(|| format!("failed to open {}", self.path.display()))?;
        writeln!(
            f,
            "{}",
            serde_json::json!({ "audio": audio_key, "text": text })
        )?;
        Ok(())
    }

    /// 全対応を (audio_key, 正解文) で返す（evaluate の入力）。
    pub(crate) fn entries(cache_dir: &Path) -> Vec<(String, String)> {
        let path = cache_dir.join("refs.jsonl");
        let mut out = Vec::new();
        let mut seen = BTreeSet::new();
        if let Ok(text) = fs::read_to_string(&path) {
            for line in text.lines() {
                let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else {
                    continue;
                };
                let (Some(a), Some(t)) = (
                    v.get("audio").and_then(|x| x.as_str()),
                    v.get("text").and_then(|x| x.as_str()),
                ) else {
                    continue;
                };
                if seen.insert(a.to_string()) {
                    out.push((a.to_string(), t.to_string()));
                }
            }
        }
        out
    }
}

/// 実行をまたいだ既処理の記録（SPEC 12 章の `seen.jsonl`）。
///
/// 1 行 1 JSON オブジェクト（`{"article":"qiita/abc"}` か
/// `{"sentence":"<sha256>"}`）。壊れた行は黙って読み飛ばす — 記録の欠落は
/// 「再処理される」方向にしか倒れず、キャッシュが高価な部分を守る。
struct Seen {
    path: PathBuf,
    articles: BTreeSet<String>,
    sentences: BTreeSet<String>,
}

impl Seen {
    fn load(cache_dir: &Path) -> Self {
        let path = cache_dir.join("seen.jsonl");
        let mut articles = BTreeSet::new();
        let mut sentences = BTreeSet::new();
        if let Ok(text) = fs::read_to_string(&path) {
            for line in text.lines() {
                let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else {
                    continue;
                };
                if let Some(a) = v.get("article").and_then(|x| x.as_str()) {
                    articles.insert(a.to_string());
                }
                if let Some(s) = v.get("sentence").and_then(|x| x.as_str()) {
                    sentences.insert(s.to_string());
                }
            }
        }
        Self {
            path,
            articles,
            sentences,
        }
    }

    fn has_article(&self, key: &str) -> bool {
        self.articles.contains(key)
    }

    fn has_sentence(&self, hash: &str) -> bool {
        self.sentences.contains(hash)
    }

    fn record_article(&mut self, key: &str) -> Result<()> {
        if self.articles.insert(key.to_string()) {
            self.append(&serde_json::json!({ "article": key }).to_string())?;
        }
        Ok(())
    }

    fn record_sentence(&mut self, hash: &str) -> Result<()> {
        if self.sentences.insert(hash.to_string()) {
            self.append(&serde_json::json!({ "sentence": hash }).to_string())?;
        }
        Ok(())
    }

    fn append(&self, line: &str) -> Result<()> {
        use std::io::Write as _;
        if let Some(dir) = self.path.parent() {
            fs::create_dir_all(dir)?;
        }
        let mut f = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
            .with_context(|| format!("failed to open {}", self.path.display()))?;
        writeln!(f, "{line}")?;
        Ok(())
    }
}

/// 表記 diff の前処理: 全角句読点を除去する。
///
/// ASR が読点・句点を即興で挿入し、置換ブロックに句読点が混ざって読み比較が
/// 崩れるのを防ぐ（Step 0 実測）。対象は観測された全角句読点のみ。半角の
/// `.`/`,` は「1.5」など数値の一部になりうるため触らない。
pub(crate) fn strip_punct(s: &str) -> String {
    s.chars()
        .filter(|c| !matches!(c, '、' | '。' | '，' | '．'))
        .collect()
}

/// 内容アドレスのキー。部品を長さプレフィックス付きで連結して SHA-256 する
/// （区切り文字が本文に現れても衝突しない）。
pub(crate) fn content_key(parts: &[&str]) -> String {
    let mut hasher = Sha256::new();
    for p in parts {
        hasher.update((p.len() as u64).to_le_bytes());
        hasher.update(p.as_bytes());
    }
    let digest = hasher.finalize();
    let mut hex = String::with_capacity(64);
    for b in digest {
        hex.push_str(&format!("{:02x}", b));
    }
    hex
}

/// 書き込み途中ファイルの一時名（`{name}.part`）。rename で完成させる。
pub(crate) fn part_path(p: &Path) -> PathBuf {
    let name = p
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    p.with_file_name(format!("{name}.part"))
}

/// キャッシュのディレクトリ配置（SPEC 12 章）。
pub(crate) struct Cache {
    audio_dir: PathBuf,
    asr_dir: PathBuf,
    asr_biased_dir: PathBuf,
}

impl Cache {
    pub(crate) fn new(dir: &Path) -> Result<Self> {
        let audio_dir = dir.join("audio");
        let asr_dir = dir.join("asr");
        let asr_biased_dir = dir.join("asr-biased");
        fs::create_dir_all(&audio_dir)
            .with_context(|| format!("failed to create {}", audio_dir.display()))?;
        fs::create_dir_all(&asr_dir)
            .with_context(|| format!("failed to create {}", asr_dir.display()))?;
        fs::create_dir_all(&asr_biased_dir)
            .with_context(|| format!("failed to create {}", asr_biased_dir.display()))?;
        Ok(Self {
            audio_dir,
            asr_dir,
            asr_biased_dir,
        })
    }

    pub(crate) fn audio_path(&self, key: &str) -> PathBuf {
        self.audio_dir.join(format!("{key}.wav"))
    }

    pub(crate) fn asr_path(&self, key: &str) -> PathBuf {
        self.asr_dir.join(format!("{key}.txt"))
    }

    /// バイアシング下の認識結果。キーには語リストの内容も入っている
    /// （`evaluate` 参照）ため、辞書が変われば別のファイルになる。
    pub(crate) fn asr_biased_path(&self, key: &str) -> PathBuf {
        self.asr_biased_dir.join(format!("{key}.txt"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::source::{Article, Body};
    use crate::token::Token;
    use std::cell::Cell;
    use std::collections::HashMap;

    // extract のフィルタ（20 字以上・日本語率・残骸なし）を通る素材。
    // 械/会 は同音（カイ）、犬/猫 は読み違い（イヌ/ネコ）。
    const REF_KAI: &str = "機械を実装して品質の確認を進める作業です";
    const HYP_KAI: &str = "機会を実装して品質の確認を進める作業です";
    const REF_INU: &str = "犬と猫を比較して品質の確認を進める作業です";
    const HYP_INU: &str = "猫と猫を比較して品質の確認を進める作業です";

    /// 行を 1 記事にまとめて返すだけのソース（ローカル入力相当・seen 不使用）。
    struct FixedSource(Vec<&'static str>);

    impl TextSource for FixedSource {
        fn fetch(&self, _n: usize) -> Result<Vec<Article>> {
            Ok(vec![Article {
                id: "fixture".into(),
                title: "fixture".into(),
                url: String::new(),
                popularity: 0,
                body: Body::Plain(self.0.join("\n")),
            }])
        }

        fn label(&self) -> &str {
            "file"
        }

        fn dedup_across_runs(&self) -> bool {
            false
        }

        fn trusted_input(&self) -> bool {
            true
        }
    }

    /// ネットソース相当（dedup_across_runs = true・記事 id を指定できる）。
    struct DedupSource {
        id: &'static str,
        text: &'static str,
    }

    impl TextSource for DedupSource {
        fn fetch(&self, _n: usize) -> Result<Vec<Article>> {
            Ok(vec![Article {
                id: self.id.into(),
                title: self.id.into(),
                url: String::new(),
                popularity: 0,
                body: Body::Plain(self.text.into()),
            }])
        }

        fn label(&self) -> &str {
            "net"
        }
    }

    /// 音声の代わりに「文そのもの」をファイルへ書くモック。呼び出し回数を数える。
    struct TextWritingSynth {
        calls: Cell<usize>,
    }

    impl TextWritingSynth {
        fn new() -> Self {
            Self { calls: Cell::new(0) }
        }
    }

    impl Synthesizer for TextWritingSynth {
        fn synth(&self, text: &str, _voice: &VoiceSpec, out: &Path) -> Result<()> {
            self.calls.set(self.calls.get() + 1);
            fs::write(out, text)?;
            Ok(())
        }
    }

    /// 「音声ファイル」（= 文テキスト）を読み、変換表に従って認識結果を返すモック。
    struct MappingRecognizer {
        map: HashMap<String, String>,
        calls: Cell<usize>,
    }

    impl MappingRecognizer {
        fn new(pairs: &[(&str, &str)]) -> Self {
            Self {
                map: pairs
                    .iter()
                    .map(|(k, v)| (k.to_string(), v.to_string()))
                    .collect(),
                calls: Cell::new(0),
            }
        }
    }

    impl Recognizer for MappingRecognizer {
        fn recognize(&self, audio: &Path, _bias: Option<&[String]>) -> Result<String> {
            self.calls.set(self.calls.get() + 1);
            let text = fs::read_to_string(audio)?;
            Ok(self.map.get(&text).cloned().unwrap_or(text))
        }
    }

    /// 1 文字 = 1 トークンの素朴トークナイザ。読みは固定表で引く。
    struct CharTokenizer;

    impl Tokenizer for CharTokenizer {
        fn tokenize(&self, text: &str) -> Result<Vec<Token>> {
            Ok(text
                .chars()
                .map(|ch| {
                    let reading = match ch {
                        '機' => "キ",
                        '械' | '会' => "カイ",
                        '実' => "ジツ",
                        '装' => "ソウ",
                        '犬' => "イヌ",
                        '猫' => "ネコ",
                        'を' => "ヲ",
                        _ => return Token::new(ch.to_string(), ch.to_string(), "名詞"),
                    };
                    Token::new(ch.to_string(), reading, "名詞")
                })
                .collect())
        }
    }

    fn voice(engine: &str, id: &str) -> VoiceSpec {
        VoiceSpec {
            engine: engine.into(),
            voice: id.into(),
            rate: 1.0,
        }
    }

    fn test_cache_dir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!(
            "biasdiff-harvest-test-{}-{}",
            std::process::id(),
            name
        ));
        let _ = fs::remove_dir_all(&d);
        d
    }

    fn opts(cache_dir: PathBuf, voices: Vec<VoiceSpec>, min_votes: usize) -> HarvestOpts {
        HarvestOpts {
            count: 10,
            voices,
            min_votes,
            cache_dir,
            normalize: NormalizeOptions::loose(),
            asr_model: "test-model".into(),
            dry_run: false,
            verbose: false,
        }
    }

    #[test]
    fn end_to_end_collects_homophone() {
        let dir = test_cache_dir("e2e");
        let source = FixedSource(vec![REF_KAI, REF_INU]);
        let synth = TextWritingSynth::new();
        let rec = MappingRecognizer::new(&[(REF_KAI, HYP_KAI), (REF_INU, HYP_INU)]);
        let deps = HarvestDeps {
            source: &source,
            synthesizer: &synth,
            recognizer: &rec,
            tokenizer: &CharTokenizer,
        };

        let report = run(&deps, &opts(dir.clone(), vec![voice("mock", "a")], 1)).unwrap();

        assert_eq!(report.sentences, 2);
        assert_eq!(report.cells, 2);
        assert_eq!(report.failures, 0);
        assert_eq!(synth.calls.get(), 2);
        assert_eq!(rec.calls.get(), 2);
        // 危険語は「械」のみ（CharTokenizer は 1 文字粒度なので置換は 械/会）。
        assert_eq!(
            report.collector.danger_words_sorted(),
            vec![("械".to_string(), 1)]
        );
        // 読み違い（犬/猫）は除外ログへ。
        assert_eq!(report.collector.reject_pairs().len(), 1);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn second_run_is_fully_cached_and_idempotent() {
        let dir = test_cache_dir("idempotent");
        let source = FixedSource(vec![REF_KAI]);
        let rec_map = [(REF_KAI, HYP_KAI)];

        let first_words;
        {
            let synth = TextWritingSynth::new();
            let rec = MappingRecognizer::new(&rec_map);
            let deps = HarvestDeps {
                source: &source,
                synthesizer: &synth,
                recognizer: &rec,
                tokenizer: &CharTokenizer,
            };
            let report = run(&deps, &opts(dir.clone(), vec![voice("mock", "a")], 1)).unwrap();
            assert_eq!(synth.calls.get(), 1);
            assert_eq!(rec.calls.get(), 1);
            assert_eq!(report.audio_cache_hits, 0);
            assert_eq!(report.asr_cache_hits, 0);
            first_words = report.collector.danger_words_sorted();
        }

        // 2 周目: ローカル入力（dedup なし）は全文が再処理対象になるが、
        // 合成も認識もキャッシュ命中で一度も呼ばれず、結果は同一（冪等）。
        let synth = TextWritingSynth::new();
        let rec = MappingRecognizer::new(&rec_map);
        let deps = HarvestDeps {
            source: &source,
            synthesizer: &synth,
            recognizer: &rec,
            tokenizer: &CharTokenizer,
        };
        let report = run(&deps, &opts(dir.clone(), vec![voice("mock", "a")], 1)).unwrap();
        assert_eq!(report.sentences, 1);
        assert_eq!(synth.calls.get(), 0);
        assert_eq!(rec.calls.get(), 0);
        assert_eq!(report.audio_cache_hits, 1);
        assert_eq!(report.asr_cache_hits, 1);
        assert_eq!(report.collector.danger_words_sorted(), first_words);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn dry_run_touches_no_engine() {
        let dir = test_cache_dir("dryrun");
        // 明示入力（trusted_input）は短い行も捨てない（Step 1 からの不変条件。
        // extract 統合時に 20 字フィルタがかかって 1 行 1 文入力を黙って
        // 捨てる退行が実際に起きた — 3 話者実験で発見）。
        let source = FixedSource(vec![REF_KAI, "短い行", REF_INU]);
        let synth = TextWritingSynth::new();
        let rec = MappingRecognizer::new(&[]);
        let deps = HarvestDeps {
            source: &source,
            synthesizer: &synth,
            recognizer: &rec,
            tokenizer: &CharTokenizer,
        };

        let mut o = opts(dir.clone(), vec![voice("mock", "a")], 1);
        o.dry_run = true;
        let report = run(&deps, &o).unwrap();

        assert_eq!(synth.calls.get(), 0);
        assert_eq!(rec.calls.get(), 0);
        assert_eq!(report.sentences, 3);
        let got = report.dry_run_sentences.unwrap();
        assert!(got.contains(&REF_KAI.to_string()));
        assert!(got.contains(&"短い行".to_string()));
        assert!(got.contains(&REF_INU.to_string()));
        // dry-run はキャッシュディレクトリも作らない。
        assert!(!dir.exists());
    }

    #[test]
    fn synth_failure_skips_cell_and_continues() {
        struct FailingSynth;
        impl Synthesizer for FailingSynth {
            fn synth(&self, text: &str, _voice: &VoiceSpec, out: &Path) -> Result<()> {
                if text.contains('犬') {
                    anyhow::bail!("boom");
                }
                fs::write(out, text)?;
                Ok(())
            }
        }

        let dir = test_cache_dir("failure");
        let source = FixedSource(vec![REF_KAI, REF_INU]);
        let rec = MappingRecognizer::new(&[(REF_KAI, HYP_KAI)]);
        let deps = HarvestDeps {
            source: &source,
            synthesizer: &FailingSynth,
            recognizer: &rec,
            tokenizer: &CharTokenizer,
        };

        let report = run(&deps, &opts(dir.clone(), vec![voice("mock", "a")], 1)).unwrap();
        // 1 セル失敗しても残りは収穫される。
        assert_eq!(report.failures, 1);
        assert_eq!(report.collector.danger_len(), 1);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn vote_threshold_drops_single_speaker_pairs() {
        let dir = test_cache_dir("vote");
        let source = FixedSource(vec![REF_KAI]);
        let synth = TextWritingSynth::new();
        let rec = MappingRecognizer::new(&[(REF_KAI, HYP_KAI)]);
        let deps = HarvestDeps {
            source: &source,
            synthesizer: &synth,
            recognizer: &rec,
            tokenizer: &CharTokenizer,
        };

        // 声は 1 話者のみ。min_votes=2 だと採用ゼロ・dropped に記録。
        let report = run(&deps, &opts(dir.clone(), vec![voice("mock", "a")], 2)).unwrap();
        assert_eq!(report.collector.danger_len(), 0);
        assert_eq!(report.vote.dropped_pairs, 1);

        // 2 話者構成なら同じペアが両話者で観測され、min_votes=2 を通る。
        let synth2 = TextWritingSynth::new();
        let rec2 = MappingRecognizer::new(&[(REF_KAI, HYP_KAI)]);
        let deps2 = HarvestDeps {
            source: &source,
            synthesizer: &synth2,
            recognizer: &rec2,
            tokenizer: &CharTokenizer,
        };
        let report2 = run(
            &deps2,
            &opts(dir.clone(), vec![voice("mock", "a"), voice("mock", "b")], 2),
        )
        .unwrap();
        assert_eq!(report2.collector.danger_len(), 1);
        assert_eq!(report2.vote.passed_pairs, 1);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn seen_skips_processed_article_on_second_run() {
        let dir = test_cache_dir("seen-article");
        let source = DedupSource {
            id: "a1",
            text: REF_KAI,
        };

        {
            let synth = TextWritingSynth::new();
            let rec = MappingRecognizer::new(&[(REF_KAI, HYP_KAI)]);
            let deps = HarvestDeps {
                source: &source,
                synthesizer: &synth,
                recognizer: &rec,
                tokenizer: &CharTokenizer,
            };
            let report = run(&deps, &opts(dir.clone(), vec![voice("mock", "a")], 1)).unwrap();
            assert_eq!(report.collector.danger_len(), 1);
            assert_eq!(report.skipped_articles, 0);
        }

        // 2 周目: 同じ記事 id は丸ごとスキップ。エンジンにもキャッシュにも触れない。
        let synth = TextWritingSynth::new();
        let rec = MappingRecognizer::new(&[(REF_KAI, HYP_KAI)]);
        let deps = HarvestDeps {
            source: &source,
            synthesizer: &synth,
            recognizer: &rec,
            tokenizer: &CharTokenizer,
        };
        let report = run(&deps, &opts(dir.clone(), vec![voice("mock", "a")], 1)).unwrap();
        assert_eq!(report.skipped_articles, 1);
        assert_eq!(report.sentences, 0);
        assert_eq!(synth.calls.get(), 0);
        assert_eq!(rec.calls.get(), 0);
        assert_eq!(report.collector.danger_len(), 0);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn seen_skips_same_sentence_from_different_article() {
        let dir = test_cache_dir("seen-sentence");
        {
            let source = DedupSource {
                id: "a1",
                text: REF_KAI,
            };
            let synth = TextWritingSynth::new();
            let rec = MappingRecognizer::new(&[(REF_KAI, HYP_KAI)]);
            let deps = HarvestDeps {
                source: &source,
                synthesizer: &synth,
                recognizer: &rec,
                tokenizer: &CharTokenizer,
            };
            run(&deps, &opts(dir.clone(), vec![voice("mock", "a")], 1)).unwrap();
        }

        // 別 id の記事が同じ文を含む（トレンドの転載・テンプレ文相当）→ 文単位で弾く。
        let source = DedupSource {
            id: "a2",
            text: REF_KAI,
        };
        let synth = TextWritingSynth::new();
        let rec = MappingRecognizer::new(&[(REF_KAI, HYP_KAI)]);
        let deps = HarvestDeps {
            source: &source,
            synthesizer: &synth,
            recognizer: &rec,
            tokenizer: &CharTokenizer,
        };
        let report = run(&deps, &opts(dir.clone(), vec![voice("mock", "a")], 1)).unwrap();
        assert_eq!(report.skipped_articles, 0);
        assert_eq!(report.skipped_sentences, 1);
        assert_eq!(report.sentences, 0);
        assert_eq!(synth.calls.get(), 0);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn failed_sentence_is_not_marked_seen_and_retries() {
        struct FailingSynth;
        impl Synthesizer for FailingSynth {
            fn synth(&self, _text: &str, _voice: &VoiceSpec, _out: &Path) -> Result<()> {
                anyhow::bail!("engine down")
            }
        }

        let dir = test_cache_dir("seen-retry");
        let source = DedupSource {
            id: "a1",
            text: REF_KAI,
        };

        {
            // 1 周目: 合成が全滅 → 文は seen に記録されない。
            let rec = MappingRecognizer::new(&[(REF_KAI, HYP_KAI)]);
            let deps = HarvestDeps {
                source: &source,
                synthesizer: &FailingSynth,
                recognizer: &rec,
                tokenizer: &CharTokenizer,
            };
            let report = run(&deps, &opts(dir.clone(), vec![voice("mock", "a")], 1)).unwrap();
            assert_eq!(report.failures, 1);
            assert_eq!(report.collector.danger_len(), 0);
        }

        // 2 周目: エンジンが直れば同じ文が再処理され、収穫される。
        let synth = TextWritingSynth::new();
        let rec = MappingRecognizer::new(&[(REF_KAI, HYP_KAI)]);
        let deps = HarvestDeps {
            source: &source,
            synthesizer: &synth,
            recognizer: &rec,
            tokenizer: &CharTokenizer,
        };
        let report = run(&deps, &opts(dir.clone(), vec![voice("mock", "a")], 1)).unwrap();
        assert_eq!(report.skipped_sentences, 0);
        assert_eq!(report.collector.danger_len(), 1);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn strip_punct_removes_fullwidth_only() {
        assert_eq!(strip_punct("し、解答率を。"), "し解答率を");
        // 半角は数値の一部かもしれないので触らない。
        assert_eq!(strip_punct("バージョン1.5,です"), "バージョン1.5,です");
    }

    #[test]
    fn content_key_separates_parts() {
        // 部品の境界が違えば別キー（長さプレフィックスの効果）。
        assert_ne!(content_key(&["ab", "c"]), content_key(&["a", "bc"]));
        assert_eq!(content_key(&["ab", "c"]), content_key(&["ab", "c"]));
    }
}

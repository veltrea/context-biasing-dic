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
use crate::pipeline::process;
use crate::reading::NormalizeOptions;
use crate::recognize::Recognizer;
use crate::source::{Body, TextSource};
use crate::synth::{Synthesizer, VoiceSpec};
use crate::token::Tokenizer;
use crate::vote::{VoteBook, VoteSummary};
use anyhow::{Context, Result};
use sha2::{Digest, Sha256};
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
    /// 例文化後の文数。
    pub sentences: usize,
    /// 処理セル数（文 × 声）。
    pub cells: usize,
    pub audio_cache_hits: usize,
    pub asr_cache_hits: usize,
    /// 合成・認識に失敗して飛ばしたセル数（失敗は run 全体を止めない）。
    pub failures: usize,
    /// dry-run のときだけ Some。例文化の結果。
    pub dry_run_sentences: Option<Vec<String>>,
}

/// harvest 一周。
pub fn run(deps: &HarvestDeps, opts: &HarvestOpts) -> Result<HarvestReport> {
    let articles = deps.source.fetch(opts.count)?;

    // 文化。Step 1 は素朴な行分割。Step 2 で extract モジュール
    // （構造除去・長さ/日本語率フィルタ・スコアリング・重複排除）に置き換える。
    let mut sentences: Vec<String> = Vec::new();
    for a in &articles {
        sentences.extend(naive_sentences(&a.body));
    }

    if opts.dry_run {
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
            dry_run_sentences: Some(sentences),
        });
    }

    let cache = Cache::new(&opts.cache_dir)?;
    let mut book = VoteBook::new();
    let mut audio_cache_hits = 0usize;
    let mut asr_cache_hits = 0usize;
    let mut failures = 0usize;
    let total = sentences.len();

    for (i, sent) in sentences.iter().enumerate() {
        for voice in &opts.voices {
            let started = Instant::now();
            // 音声: 内容アドレス（文 + エンジン + 話者 + 話速）。
            let audio_key = content_key(&[sent, &voice.engine, &voice.voice, &voice.rate_key()]);
            let audio_path = cache.audio_path(&audio_key);
            let audio_was_cached = audio_path.exists();
            if !audio_was_cached {
                let part = part_path(&audio_path);
                if let Err(e) = deps.synthesizer.synth(sent, voice, &part) {
                    failures += 1;
                    let _ = fs::remove_file(&part);
                    eprintln!("warn: synth failed ({}): {e:#}", voice.speaker_key());
                    continue;
                }
                fs::rename(&part, &audio_path)
                    .with_context(|| format!("failed to move {} into cache", part.display()))?;
            } else {
                audio_cache_hits += 1;
            }

            // 認識: 音声キー + モデル名（モデルが違えば別の結果）。
            let asr_key = content_key(&[&audio_key, &opts.asr_model]);
            let asr_path = cache.asr_path(&asr_key);
            let asr_was_cached = asr_path.exists();
            let hypothesis = if asr_was_cached {
                asr_cache_hits += 1;
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
                        eprintln!("warn: asr failed ({}): {e:#}", audio_path.display());
                        continue;
                    }
                }
            };

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
                    i + 1,
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
        dry_run_sentences: None,
    })
}

/// Step 1 の素朴な文化: 行で割って trim、空行を捨てる。
/// 形式（Markdown/HTML）の構造除去は Step 2 の extract が担う。
fn naive_sentences(body: &Body) -> Vec<String> {
    let text = match body {
        Body::Markdown(s) | Body::Html(s) | Body::Plain(s) => s,
    };
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(String::from)
        .collect()
}

/// 表記 diff の前処理: 全角句読点を除去する。
///
/// ASR が読点・句点を即興で挿入し、置換ブロックに句読点が混ざって読み比較が
/// 崩れるのを防ぐ（Step 0 実測）。対象は観測された全角句読点のみ。半角の
/// `.`/`,` は「1.5」など数値の一部になりうるため触らない。
fn strip_punct(s: &str) -> String {
    s.chars()
        .filter(|c| !matches!(c, '、' | '。' | '，' | '．'))
        .collect()
}

/// 内容アドレスのキー。部品を長さプレフィックス付きで連結して SHA-256 する
/// （区切り文字が本文に現れても衝突しない）。
fn content_key(parts: &[&str]) -> String {
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
fn part_path(p: &Path) -> PathBuf {
    let name = p
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    p.with_file_name(format!("{name}.part"))
}

/// キャッシュのディレクトリ配置（SPEC 12 章）。
struct Cache {
    audio_dir: PathBuf,
    asr_dir: PathBuf,
}

impl Cache {
    fn new(dir: &Path) -> Result<Self> {
        let audio_dir = dir.join("audio");
        let asr_dir = dir.join("asr");
        fs::create_dir_all(&audio_dir)
            .with_context(|| format!("failed to create {}", audio_dir.display()))?;
        fs::create_dir_all(&asr_dir)
            .with_context(|| format!("failed to create {}", asr_dir.display()))?;
        Ok(Self { audio_dir, asr_dir })
    }

    fn audio_path(&self, key: &str) -> PathBuf {
        self.audio_dir.join(format!("{key}.wav"))
    }

    fn asr_path(&self, key: &str) -> PathBuf {
        self.asr_dir.join(format!("{key}.txt"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::source::Article;
    use crate::token::Token;
    use std::cell::Cell;
    use std::collections::HashMap;

    /// 行を 1 記事にまとめて返すだけのソース。
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
        let source = FixedSource(vec!["機械を実装", "犬を実装"]);
        let synth = TextWritingSynth::new();
        // 「械→会」は同音（カイ=カイ）、「犬→猫」は読み違い。
        let rec = MappingRecognizer::new(&[("機械を実装", "機会を実装"), ("犬を実装", "猫を実装")]);
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
        let source = FixedSource(vec!["機械を実装"]);
        let rec_map = [("機械を実装", "機会を実装")];

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

        // 2 周目: 合成も認識も一度も呼ばれず、結果は同一（冪等）。
        let synth = TextWritingSynth::new();
        let rec = MappingRecognizer::new(&rec_map);
        let deps = HarvestDeps {
            source: &source,
            synthesizer: &synth,
            recognizer: &rec,
            tokenizer: &CharTokenizer,
        };
        let report = run(&deps, &opts(dir.clone(), vec![voice("mock", "a")], 1)).unwrap();
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
        let source = FixedSource(vec!["機械を実装", "", "  ", "犬を実装"]);
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
        // 空行・空白行は文に数えない。
        assert_eq!(report.sentences, 2);
        assert_eq!(
            report.dry_run_sentences,
            Some(vec!["機械を実装".to_string(), "犬を実装".to_string()])
        );
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
        let source = FixedSource(vec!["機械を実装", "犬を実装"]);
        let rec = MappingRecognizer::new(&[("機械を実装", "機会を実装")]);
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
        let source = FixedSource(vec!["機械を実装"]);
        let synth = TextWritingSynth::new();
        let rec = MappingRecognizer::new(&[("機械を実装", "機会を実装")]);
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
        let rec2 = MappingRecognizer::new(&[("機械を実装", "機会を実装")]);
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

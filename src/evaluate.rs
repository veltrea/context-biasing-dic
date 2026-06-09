//! `evaluate`: 辞書をコンテキストバイアシングとして投入し、語数 N を増やし
//! ながら同じ音声セットを再認識して、衝突率の頭打ち点を見つける（SPEC 14 章）。
//! v0.1 設計書 7 章「辞書サイズは理論ではなく実測の頭打ちで決める」の自動化。
//!
//! 入力は harvest が残した `refs.jsonl`（音声キー → 正解文）とキャッシュ済み
//! 音声。*衝突*の定義は収穫時と同じ — 既存分類器（`pipeline::process`）が
//! 採用した同音置換の数。辞書を作ったコードがそのまま指標を計算する。

use crate::harvest::{content_key, strip_punct, Cache, Refs};
use crate::pipeline::process;
use crate::reading::NormalizeOptions;
use crate::recognize::Recognizer;
use crate::token::Tokenizer;
use anyhow::{bail, Context, Result};
use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;

pub struct EvaluateDeps<'a> {
    pub recognizer: &'a dyn Recognizer,
    pub tokenizer: &'a dyn Tokenizer,
}

pub struct EvaluateOpts {
    /// 試験する辞書（count 降順で並んだ語）。
    pub words: Vec<String>,
    pub cache_dir: PathBuf,
    /// ASR モデル名（キャッシュキーの一部）。
    pub asr_model: String,
    /// N の刻み。
    pub step: usize,
    /// N の上限。
    pub max_words: usize,
    /// 頭打ち判定: 1 ステップの衝突率改善がこれ未満なら「改善なし」。
    pub min_delta: f64,
    /// 「改善なし」が連続この回数続いたら頭打ち。
    pub patience: usize,
    pub normalize: NormalizeOptions,
    pub verbose: bool,
}

/// カーブ上の 1 点。
#[derive(Debug, Clone, PartialEq)]
pub struct EvaluatePoint {
    pub n: usize,
    pub collisions: usize,
    /// collisions / 音声数。
    pub rate: f64,
}

pub struct EvaluateReport {
    pub points: Vec<EvaluatePoint>,
    /// 頭打ちと判定した最小の N。検出できなければ None（まだ改善中 or 効果なし）。
    pub recommended_n: Option<usize>,
    /// 評価に使った音声数。
    pub sentences: usize,
    /// キャッシュ外で実際に走らせた認識回数。
    pub asr_runs: usize,
    pub failures: usize,
    /// 実際に衝突を直した語（N=0 で衝突 ∧ 最終 N で衝突せず ∧ 辞書の top-N 内）。
    /// `--prune` の出力。
    pub pruned: Vec<String>,
}

pub fn run(deps: &EvaluateDeps, opts: &EvaluateOpts) -> Result<EvaluateReport> {
    let entries = Refs::entries(&opts.cache_dir);
    if entries.is_empty() {
        bail!(
            "no refs.jsonl under {} — run `biasdiff harvest` first to build the audio set",
            opts.cache_dir.display()
        );
    }
    let cache = Cache::new(&opts.cache_dir)?;

    // N スケジュール: 0, step, 2*step, ... に辞書サイズ（または max-words）の
    // 終端を足す。終端が刻みと重なれば重複は除く。
    let cap = opts.words.len().min(opts.max_words);
    let mut schedule: Vec<usize> = Vec::new();
    let mut n = 0;
    while n < cap {
        schedule.push(n);
        n += opts.step.max(1);
    }
    schedule.push(cap);
    schedule.dedup();

    let mut points: Vec<EvaluatePoint> = Vec::new();
    // N ごとの「衝突した正解表記」集合。prune（実際に直した語）の材料。
    let mut collided_at: Vec<(usize, BTreeSet<String>)> = Vec::new();
    let mut asr_runs = 0usize;
    let mut failures = 0usize;

    for &n in &schedule {
        let bias: Vec<String> = opts.words.iter().take(n).cloned().collect();
        let mut collisions = 0usize;
        let mut collided: BTreeSet<String> = BTreeSet::new();

        for (audio_key, reference) in &entries {
            let audio_path = cache.audio_path(audio_key);
            if !audio_path.exists() {
                // 音声キャッシュが消えている（手動削除など）。再合成は harvest の
                // 責務なので、ここでは数えて飛ばすだけ。
                failures += 1;
                continue;
            }

            // N=0（bias なし）は harvest の asr/ キャッシュとキーが一致し、
            // 収穫済みならゼロコスト。bias ありは語リストの内容ごとに別キー —
            // 辞書が変われば同じ N でも別の結果として扱う。
            let (path, bias_arg) = if n == 0 {
                (cache.asr_path(&content_key(&[audio_key, &opts.asr_model])), None)
            } else {
                let key = content_key(&[audio_key, &opts.asr_model, &bias.join("\u{1f}")]);
                (cache.asr_biased_path(&key), Some(bias.as_slice()))
            };

            let hypothesis = if path.exists() {
                fs::read_to_string(&path)
                    .with_context(|| format!("failed to read {}", path.display()))?
            } else {
                match deps.recognizer.recognize(&audio_path, bias_arg) {
                    Ok(h) => {
                        asr_runs += 1;
                        let h = h.trim().to_string();
                        let part = crate::harvest::part_path(&path);
                        fs::write(&part, &h)
                            .with_context(|| format!("failed to write {}", part.display()))?;
                        fs::rename(&part, &path)?;
                        h
                    }
                    Err(e) => {
                        failures += 1;
                        eprintln!("warn: asr failed ({}): {e:#}", audio_path.display());
                        continue;
                    }
                }
            };

            let outcomes = process(
                deps.tokenizer,
                &strip_punct(reference),
                &strip_punct(&hypothesis),
                &opts.normalize,
            )?;
            for o in &outcomes {
                if o.is_homophone() {
                    collisions += 1;
                    collided.insert(o.candidate().reference_surface.clone());
                }
            }
        }

        let rate = collisions as f64 / entries.len() as f64;
        if opts.verbose {
            eprintln!(
                "evaluate: N={:<4} collisions={:<4} rate={:.4}",
                n, collisions, rate
            );
        }
        points.push(EvaluatePoint { n, collisions, rate });
        collided_at.push((n, collided));
    }

    let recommended_n = find_plateau(&points, opts.min_delta, opts.patience);

    // prune: 「N=0 で衝突した ∧ 最終 N の biasing 下では衝突しなかった」語の
    // うち辞書の top-N に入っているもの = コンテキスト予算のコストに見合うと
    // 実証された語。
    let final_n = recommended_n.unwrap_or_else(|| points.last().map(|p| p.n).unwrap_or(0));
    let empty = BTreeSet::new();
    let c0 = collided_at
        .iter()
        .find(|(n, _)| *n == 0)
        .map(|(_, s)| s)
        .unwrap_or(&empty);
    let cf = collided_at
        .iter()
        .find(|(n, _)| *n == final_n)
        .map(|(_, s)| s)
        .unwrap_or(&empty);
    let top: BTreeSet<&String> = opts.words.iter().take(final_n).collect();
    let pruned: Vec<String> = opts
        .words
        .iter()
        .take(final_n)
        .filter(|w| c0.contains(*w) && !cf.contains(*w) && top.contains(w))
        .cloned()
        .collect();

    Ok(EvaluateReport {
        points,
        recommended_n,
        sentences: entries.len(),
        asr_runs,
        failures,
        pruned,
    })
}

/// 頭打ち判定: 改善量（rate の減少）が `min_delta` 未満のステップが
/// `patience` 回連続したら、その直前の点の N を返す（それ以上足しても
/// 効かない最小の N）。
fn find_plateau(points: &[EvaluatePoint], min_delta: f64, patience: usize) -> Option<usize> {
    if points.len() < 2 || patience == 0 {
        return None;
    }
    for i in 1..points.len() {
        if i + patience > points.len() {
            break;
        }
        let flat = (i..i + patience).all(|j| points[j - 1].rate - points[j].rate < min_delta);
        if flat {
            return Some(points[i - 1].n);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::token::Token;
    use std::cell::Cell;
    use std::path::Path;

    /// bias に「械」が入っていれば正しく聞き取れるようになる認識器。
    struct BiasSensitiveRecognizer {
        calls: Cell<usize>,
    }

    impl Recognizer for BiasSensitiveRecognizer {
        fn recognize(&self, _audio: &Path, bias: Option<&[String]>) -> Result<String> {
            self.calls.set(self.calls.get() + 1);
            let fixed = bias
                .map(|b| b.iter().any(|w| w == "械"))
                .unwrap_or(false);
            Ok(if fixed {
                "機械を実装".to_string()
            } else {
                "機会を実装".to_string()
            })
        }
    }

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
                        'を' => "ヲ",
                        _ => return Token::new(ch.to_string(), ch.to_string(), "名詞"),
                    };
                    Token::new(ch.to_string(), reading, "名詞")
                })
                .collect())
        }
    }

    fn setup_cache(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "biasdiff-evaluate-test-{}-{}",
            std::process::id(),
            name
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("audio")).unwrap();
        // 音声 1 本 + 正解文。音声の中身はモックが見ないので空でよい。
        fs::write(dir.join("audio").join("k1.wav"), b"").unwrap();
        fs::write(
            dir.join("refs.jsonl"),
            "{\"audio\":\"k1\",\"text\":\"機械を実装\"}\n",
        )
        .unwrap();
        dir
    }

    fn opts(cache_dir: PathBuf, words: Vec<&str>, step: usize) -> EvaluateOpts {
        EvaluateOpts {
            words: words.into_iter().map(String::from).collect(),
            cache_dir,
            asr_model: "test-model".into(),
            step,
            max_words: 100,
            min_delta: 0.001,
            patience: 1,
            normalize: NormalizeOptions::loose(),
            verbose: false,
        }
    }

    #[test]
    fn collision_rate_drops_and_plateau_is_found() {
        let dir = setup_cache("plateau");
        let rec = BiasSensitiveRecognizer { calls: Cell::new(0) };
        let deps = EvaluateDeps {
            recognizer: &rec,
            tokenizer: &CharTokenizer,
        };

        // 辞書: 効く語「械」と無関係な語「犬」。schedule = [0, 1, 2]
        let report = run(&deps, &opts(dir.clone(), vec!["械", "犬"], 1)).unwrap();

        assert_eq!(report.sentences, 1);
        let rates: Vec<(usize, usize)> =
            report.points.iter().map(|p| (p.n, p.collisions)).collect();
        // N=0: 衝突 1（械←会）。N=1: bias=[械] で直って 0。N=2: 0 のまま。
        assert_eq!(rates, vec![(0, 1), (1, 0), (2, 0)]);
        // N=1→2 が改善なし → 頭打ち。推奨はその直前の N=1。
        assert_eq!(report.recommended_n, Some(1));
        // 実際に直した語は「械」だけ。
        assert_eq!(report.pruned, vec!["械".to_string()]);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn biased_results_are_cached_per_dictionary_content() {
        let dir = setup_cache("cache");
        {
            let rec = BiasSensitiveRecognizer { calls: Cell::new(0) };
            let deps = EvaluateDeps {
                recognizer: &rec,
                tokenizer: &CharTokenizer,
            };
            run(&deps, &opts(dir.clone(), vec!["械"], 1)).unwrap();
            // schedule [0, 1] → 認識 2 回（N=0 と N=1）。
            assert_eq!(rec.calls.get(), 2);
        }

        // 同じ辞書での再実行はすべてキャッシュ命中。
        let rec = BiasSensitiveRecognizer { calls: Cell::new(0) };
        let deps = EvaluateDeps {
            recognizer: &rec,
            tokenizer: &CharTokenizer,
        };
        let report = run(&deps, &opts(dir.clone(), vec!["械"], 1)).unwrap();
        assert_eq!(rec.calls.get(), 0);
        assert_eq!(report.asr_runs, 0);

        // 辞書の中身が変われば同じ N でも再認識される（キーに語リストが入る）。
        let rec2 = BiasSensitiveRecognizer { calls: Cell::new(0) };
        let deps2 = EvaluateDeps {
            recognizer: &rec2,
            tokenizer: &CharTokenizer,
        };
        run(&deps2, &opts(dir.clone(), vec!["犬"], 1)).unwrap();
        // N=0 は共通キャッシュ、N=1（bias=[犬]）だけ新規。
        assert_eq!(rec2.calls.get(), 1);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn missing_refs_is_an_error() {
        let dir = std::env::temp_dir().join(format!(
            "biasdiff-evaluate-test-{}-norefs",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        let rec = BiasSensitiveRecognizer { calls: Cell::new(0) };
        let deps = EvaluateDeps {
            recognizer: &rec,
            tokenizer: &CharTokenizer,
        };
        assert!(run(&deps, &opts(dir.clone(), vec!["械"], 1)).is_err());
    }

    #[test]
    fn plateau_detection_edge_cases() {
        let p = |n: usize, rate: f64| EvaluatePoint {
            n,
            collisions: 0,
            rate,
        };
        // 単調改善が続く → 頭打ちなし。
        assert_eq!(
            find_plateau(&[p(0, 0.9), p(25, 0.5), p(50, 0.1)], 0.01, 2),
            None
        );
        // 最初から平坦 → N=0 を推奨（辞書が効いていない、という情報も含めて）。
        assert_eq!(
            find_plateau(&[p(0, 0.5), p(25, 0.5), p(50, 0.5)], 0.01, 2),
            Some(0)
        );
        // 改善 → 平坦 2 連続 → 改善が止まった点の N。
        // （0.005 の微改善は min_delta=0.01 未満なので「改善なし」扱い）
        assert_eq!(
            find_plateau(
                &[p(0, 0.9), p(25, 0.3), p(50, 0.295), p(75, 0.295)],
                0.01,
                2
            ),
            Some(25)
        );
    }
}

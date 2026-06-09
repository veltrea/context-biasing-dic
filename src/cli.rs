//! コマンドライン界面。`batch`（ファイル一括）と `repl`（対話ループ）。
//!
//! UI は薄く被せるだけ。実処理はコア層（`pipeline`・`collect`）へ委ねる。

use crate::collect::Collector;
use crate::messages::msg;
use crate::morph::{DictKind, LinderaTokenizer};
use crate::pipeline::process;
use crate::reading::NormalizeOptions;
use anyhow::{Context, Result};
use clap::{Parser, Subcommand, ValueEnum};
use std::fs::{self, File};
use std::io::{self, BufRead, BufWriter, Write};
use std::path::{Path, PathBuf};

#[cfg(feature = "harvest")]
use crate::{
    asr_qwen3_mlx::Qwen3MlxRecognizer,
    harvest::{self, HarvestDeps, HarvestOpts},
    recognize::Recognizer,
    source::TextSource,
    source_file::FileSource,
    synth::{Synthesizer, VoiceSpec},
    synth_say::SaySynth,
    synth_voicevox::VoicevoxSynth,
};

#[derive(Parser)]
#[command(
    name = "biasdiff",
    version,
    about = "Diff-based homophone collector for ASR context-biasing dictionaries"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,

    /// 使用する辞書（ビルドに埋め込んだもの）。
    #[arg(long, value_enum, default_value_t = DictArg::default(), global = true)]
    dict: DictArg,

    /// 読みゆれの正規化を無効化し、完全一致で判定する。
    #[arg(long, global = true)]
    strict: bool,
}

#[derive(Subcommand)]
enum Command {
    /// 正解文ファイルと認識文ファイルを行対応で diff する。
    Batch {
        /// 正解文ファイル（1行1文）。
        #[arg(long)]
        reference: PathBuf,
        /// 認識結果ファイル（1行1文、正解文と行対応）。
        #[arg(long)]
        hypothesis: PathBuf,
        /// 危険語リストの出力先（省略時は標準出力）。
        #[arg(long, short)]
        output: Option<PathBuf>,
        /// 読み不一致ペアの別ログ出力先。
        #[arg(long)]
        reject: Option<PathBuf>,
        /// 出力形式（txt=語のみ / counts=`語\t回数` / amical-json=Amical 取り込み用 JSON）。
        /// 既定は txt。`--counts` と併用した場合は `--format` を優先する。
        #[arg(long, value_enum)]
        format: Option<OutputFormat>,
        /// 後方互換: `--format counts` と同等（`語\t回数`）。`--format` 明示時はそちらを優先。
        #[arg(long)]
        counts: bool,
        /// `amical-json` 出力時の分野ラベル（JSON の `field`）。
        #[arg(long, default_value = "general")]
        field: String,
    },
    /// 対話ループ：正解文→認識結果を貼り、即時 diff して蓄積する。
    Repl {
        /// 危険語リストの保存先（ペア処理ごとに上書き保存）。
        #[arg(long, short)]
        output: Option<PathBuf>,
        /// 読み不一致ペアの別ログ保存先。
        #[arg(long)]
        reject: Option<PathBuf>,
        /// 出力形式（txt=語のみ / counts=`語\t回数` / amical-json=Amical 取り込み用 JSON）。
        /// 既定は txt。`--counts` と併用した場合は `--format` を優先する。
        #[arg(long, value_enum)]
        format: Option<OutputFormat>,
        /// 後方互換: `--format counts` と同等（`語\t回数`）。`--format` 明示時はそちらを優先。
        #[arg(long)]
        counts: bool,
        /// `amical-json` 出力時の分野ラベル（JSON の `field`）。
        #[arg(long, default_value = "general")]
        field: String,
    },
    /// 自動収穫：取得 → TTS → ASR → diff → 辞書（v0.2、要 --features harvest）。
    #[cfg(feature = "harvest")]
    Harvest {
        /// テキストソース。Step 1 は file のみ（qiita / zenn は Step 2）。
        #[arg(long, default_value = "file")]
        source: String,
        /// file ソースの入力テキスト（1 行 1 文）。
        #[arg(long)]
        input: PathBuf,
        /// ソースから取得する記事数の上限。
        #[arg(long, default_value_t = 100)]
        count: usize,
        /// TTS エンジン（voicevox | say）。
        #[arg(long, default_value = "voicevox")]
        tts: String,
        /// VOICEVOX エンジンの URL。
        #[arg(long, default_value = "http://127.0.0.1:50021")]
        voicevox_url: String,
        /// 話者（カンマ区切り）。voicevox は speaker id、say は声名（Kyoko 等）。
        #[arg(long, value_delimiter = ',', default_value = "3")]
        voices: Vec<String>,
        /// 話速倍率（カンマ区切り。1.0 = エンジン既定）。
        #[arg(long, value_delimiter = ',', default_value = "1.0")]
        rates: Vec<f32>,
        /// ASR エンジン（qwen3-mlx のみ）。
        #[arg(long, default_value = "qwen3-mlx")]
        asr: String,
        /// ASR モデル。
        #[arg(long, default_value = "mlx-community/Qwen3-ASR-0.6B-8bit")]
        model: String,
        /// mlx-audio が入った環境の python（venv を activate して起動すれば既定で可）。
        #[arg(long, default_value = "python3")]
        asr_python: PathBuf,
        /// キャッシュディレクトリ（音声・認識結果。git 管理外）。
        #[arg(long, default_value = "./harvest_cache")]
        cache_dir: PathBuf,
        /// 採用に必要な異なり話者数。省略時: 話者 2 以上の構成なら 2、それ以外は 1。
        #[arg(long)]
        min_votes: Option<usize>,
        /// 例文化で止めて文を表示する（TTS / ASR を回さない）。
        #[arg(long)]
        dry_run: bool,
        /// 危険語リストの出力先（省略時は標準出力）。
        #[arg(long, short)]
        output: Option<PathBuf>,
        /// 読み不一致ペアの別ログ出力先。
        #[arg(long)]
        reject: Option<PathBuf>,
        /// 出力形式（txt=語のみ / counts=`語\t回数` / amical-json=Amical 取り込み用 JSON）。
        #[arg(long, value_enum)]
        format: Option<OutputFormat>,
        /// 後方互換: `--format counts` と同等。
        #[arg(long)]
        counts: bool,
        /// `amical-json` 出力時の分野ラベル（JSON の `field`）。
        #[arg(long, default_value = "general")]
        field: String,
    },
}

#[derive(Copy, Clone, ValueEnum)]
enum DictArg {
    Ipadic,
    Unidic,
}

impl Default for DictArg {
    fn default() -> Self {
        // 埋め込まれている辞書をデフォルトにする。両方あれば IPADIC を優先。
        #[cfg(feature = "ipadic")]
        {
            DictArg::Ipadic
        }
        #[cfg(all(feature = "unidic", not(feature = "ipadic")))]
        {
            DictArg::Unidic
        }
        #[cfg(not(any(feature = "ipadic", feature = "unidic")))]
        {
            DictArg::Ipadic
        }
    }
}

impl From<DictArg> for DictKind {
    fn from(a: DictArg) -> Self {
        match a {
            DictArg::Ipadic => DictKind::Ipadic,
            DictArg::Unidic => DictKind::Unidic,
        }
    }
}

/// 危険語リストの出力形式。clap が variant 名をケバブケースの値に変換する
/// （Txt→`txt`、Counts→`counts`、AmicalJson→`amical-json`）。
#[derive(Copy, Clone, Debug, PartialEq, Eq, ValueEnum)]
enum OutputFormat {
    /// 1行1語（語のみ）。そのまま ASR の用語集に投入できる。
    Txt,
    /// `語\t回数`。解析・停止条件の見極め用。
    Counts,
    /// Amical 取り込み用のバイアシング辞書（メタ付き JSON）。
    AmicalJson,
}

/// 危険語リストをどう書き出すかの指定。出力形式 `format` と、その分野ラベル
/// `field`（`amical-json` のときだけ使う）。関数の引数を増やしすぎないよう束ねる。
#[derive(Copy, Clone)]
struct OutputSpec<'a> {
    format: OutputFormat,
    field: &'a str,
}

/// `--format` と後方互換の `--counts` から、実効の出力形式を決める。
///
/// `--format` を明示したらそれを優先し、無指定なら `--counts` で counts、
/// どちらも無ければ txt。
fn resolve_format(format: Option<OutputFormat>, counts: bool) -> OutputFormat {
    match format {
        Some(f) => f,
        None if counts => OutputFormat::Counts,
        None => OutputFormat::Txt,
    }
}

/// エントリポイント。引数を解釈し、サブコマンドへ振り分ける。
pub fn run() -> Result<()> {
    let cli = Cli::parse();
    let opts = if cli.strict {
        NormalizeOptions::strict()
    } else {
        NormalizeOptions::loose()
    };
    let kind: DictKind = cli.dict.into();
    let tokenizer = LinderaTokenizer::new(kind)?;

    match cli.command {
        Command::Batch {
            reference,
            hypothesis,
            output,
            reject,
            format,
            counts,
            field,
        } => {
            let spec = OutputSpec {
                format: resolve_format(format, counts),
                field: &field,
            };
            run_batch(
                &tokenizer,
                &opts,
                &reference,
                &hypothesis,
                output.as_deref(),
                reject.as_deref(),
                spec,
            )
        }
        Command::Repl {
            output,
            reject,
            format,
            counts,
            field,
        } => {
            let spec = OutputSpec {
                format: resolve_format(format, counts),
                field: &field,
            };
            run_repl(&tokenizer, &opts, output.as_deref(), reject.as_deref(), spec)
        }
        #[cfg(feature = "harvest")]
        Command::Harvest {
            source,
            input,
            count,
            tts,
            voicevox_url,
            voices,
            rates,
            asr,
            model,
            asr_python,
            cache_dir,
            min_votes,
            dry_run,
            output,
            reject,
            format,
            counts,
            field,
        } => {
            let spec = OutputSpec {
                format: resolve_format(format, counts),
                field: &field,
            };
            let args = HarvestArgs {
                source,
                input,
                count,
                tts,
                voicevox_url,
                voices,
                rates,
                asr,
                model,
                asr_python,
                cache_dir,
                min_votes,
                dry_run,
            };
            run_harvest(
                &tokenizer,
                &opts,
                args,
                output.as_deref(),
                reject.as_deref(),
                spec,
            )
        }
    }
}

/// `harvest` の CLI 引数（出力系を除く）。`run_harvest` の引数を束ねる。
#[cfg(feature = "harvest")]
struct HarvestArgs {
    source: String,
    input: PathBuf,
    count: usize,
    tts: String,
    voicevox_url: String,
    voices: Vec<String>,
    rates: Vec<f32>,
    asr: String,
    model: String,
    asr_python: PathBuf,
    cache_dir: PathBuf,
    min_votes: Option<usize>,
    dry_run: bool,
}

/// 自動収穫一周。アダプタを組み立て、オーケストレーション（`harvest::run`）へ。
#[cfg(feature = "harvest")]
fn run_harvest(
    tokenizer: &LinderaTokenizer,
    opts: &NormalizeOptions,
    args: HarvestArgs,
    output: Option<&Path>,
    reject: Option<&Path>,
    spec: OutputSpec,
) -> Result<()> {
    // ソース。Step 1 は file のみ（qiita / zenn は Step 2 で追加）。
    let source: Box<dyn TextSource> = match args.source.as_str() {
        "file" => Box::new(FileSource::new(&args.input)),
        other => anyhow::bail!(msg!(
            format!("source '{other}' is not implemented yet (only 'file' for now)"),
            format!("ソース '{other}' は未実装です（現在は 'file' のみ）"),
        )),
    };

    // TTS。VOICEVOX は最初に疎通を確かめ、つながらないなら文を処理する前に止まる
    // （dry-run はエンジンに触れないので確認しない — VOICEVOX 不在でも使える）。
    let synthesizer: Box<dyn Synthesizer> = match args.tts.as_str() {
        "voicevox" => {
            let s = VoicevoxSynth::new(args.voicevox_url.clone());
            if !args.dry_run {
                let version = s.check()?;
                eprintln!(
                    "{}",
                    msg!(
                        format!("VOICEVOX {} at {}", version, args.voicevox_url),
                        format!("VOICEVOX {}（{}）", version, args.voicevox_url),
                    )
                );
            }
            Box::new(s)
        }
        "say" => Box::new(SaySynth),
        other => anyhow::bail!(msg!(
            format!("unknown TTS engine '{other}' (voicevox | say)"),
            format!("TTS エンジン '{other}' は未対応です（voicevox | say）"),
        )),
    };

    let recognizer: Box<dyn Recognizer> = match args.asr.as_str() {
        "qwen3-mlx" => Box::new(Qwen3MlxRecognizer::new(&args.asr_python, args.model.clone())),
        other => anyhow::bail!(msg!(
            format!("unknown ASR engine '{other}' (qwen3-mlx)"),
            format!("ASR エンジン '{other}' は未対応です（qwen3-mlx）"),
        )),
    };

    // 声マトリクス（話者 × 話速）。
    let mut voice_specs: Vec<VoiceSpec> = Vec::new();
    for v in &args.voices {
        for r in &args.rates {
            voice_specs.push(VoiceSpec {
                engine: args.tts.clone(),
                voice: v.clone(),
                rate: *r,
            });
        }
    }

    // min-votes の既定（SPEC 11 章): 異なり話者 2 以上の構成なら 2、それ以外は 1。
    let distinct_speakers: std::collections::BTreeSet<String> =
        voice_specs.iter().map(|v| v.speaker_key()).collect();
    let min_votes = args
        .min_votes
        .unwrap_or(if distinct_speakers.len() >= 2 { 2 } else { 1 });

    let deps = HarvestDeps {
        source: source.as_ref(),
        synthesizer: synthesizer.as_ref(),
        recognizer: recognizer.as_ref(),
        tokenizer,
    };
    let hopts = HarvestOpts {
        count: args.count,
        voices: voice_specs,
        min_votes,
        cache_dir: args.cache_dir,
        normalize: *opts,
        asr_model: args.model,
        dry_run: args.dry_run,
        verbose: true,
    };

    let report = harvest::run(&deps, &hopts)?;

    // dry-run: 例文を標準出力へ出して終わり（パイプで覗ける）。
    if let Some(sentences) = report.dry_run_sentences {
        for s in &sentences {
            println!("{}", s);
        }
        eprintln!(
            "{}",
            msg!(
                format!("{} sentence(s) extracted (dry run).", sentences.len()),
                format!("例文 {} 文を抽出しました（dry run）。", sentences.len()),
            )
        );
        return Ok(());
    }

    emit_dict(&report.collector, output, spec)?;

    if let Some(p) = reject {
        let mut w = BufWriter::new(
            File::create(p).with_context(|| format!("failed to create {}", p.display()))?,
        );
        report.collector.write_reject(&mut w)?;
        w.flush()?;
    }

    // 統計は stderr へ（標準出力の辞書をパイプで汚さない）。
    eprintln!(
        "{}",
        msg!(
            format!(
                "done: {} sentence(s) x {} voice(s), cache hits audio {}/{} asr {}/{}, {} failure(s).",
                report.sentences,
                if report.sentences == 0 { 0 } else { report.cells / report.sentences },
                report.audio_cache_hits,
                report.cells,
                report.asr_cache_hits,
                report.cells,
                report.failures
            ),
            format!(
                "処理: {} 文 × {} 声、キャッシュ命中 音声 {}/{} 認識 {}/{}、失敗 {} 件。",
                report.sentences,
                if report.sentences == 0 { 0 } else { report.cells / report.sentences },
                report.audio_cache_hits,
                report.cells,
                report.asr_cache_hits,
                report.cells,
                report.failures
            ),
        )
    );
    if report.vote.dropped_pairs > 0 {
        eprintln!(
            "{}",
            msg!(
                format!(
                    "vote: {} pair(s) adopted, {} dropped (fewer than {} distinct speakers).",
                    report.vote.passed_pairs, report.vote.dropped_pairs, min_votes
                ),
                format!(
                    "投票: 採用 {} ペア、棄却 {} ペア（異なり話者 {} 未満）。",
                    report.vote.passed_pairs, report.vote.dropped_pairs, min_votes
                ),
            )
        );
    }
    eprintln!(
        "{}",
        msg!(
            format!(
                "done: {} danger word(s), {} rejected pair(s).",
                report.collector.danger_len(),
                report.collector.reject_pairs().len()
            ),
            format!(
                "完了: 危険語 {} 語、除外ペア {} 件。",
                report.collector.danger_len(),
                report.collector.reject_pairs().len()
            ),
        )
    );
    Ok(())
}

/// 正解文・認識文ファイルを行対応で照合する。
fn run_batch(
    tokenizer: &LinderaTokenizer,
    opts: &NormalizeOptions,
    reference: &Path,
    hypothesis: &Path,
    output: Option<&Path>,
    reject: Option<&Path>,
    spec: OutputSpec,
) -> Result<()> {
    let ref_text = fs::read_to_string(reference)
        .with_context(|| format!("failed to read {}", reference.display()))?;
    let hyp_text = fs::read_to_string(hypothesis)
        .with_context(|| format!("failed to read {}", hypothesis.display()))?;

    let ref_lines: Vec<&str> = ref_text.lines().collect();
    let hyp_lines: Vec<&str> = hyp_text.lines().collect();
    if ref_lines.len() != hyp_lines.len() {
        eprintln!(
            "{}",
            msg!(
                format!(
                    "warning: line count differs (reference {}, hypothesis {}); pairing up to the shorter.",
                    ref_lines.len(),
                    hyp_lines.len()
                ),
                format!(
                    "警告: 行数が違います（正解 {} / 認識 {}）。短い方に合わせて対応します。",
                    ref_lines.len(),
                    hyp_lines.len()
                ),
            )
        );
    }

    let mut collector = Collector::new();
    for (r, h) in ref_lines.iter().zip(hyp_lines.iter()) {
        if r.trim().is_empty() && h.trim().is_empty() {
            continue;
        }
        let outs = process(tokenizer, r, h, opts)?;
        collector.add_all(outs);
    }

    emit_dict(&collector, output, spec)?;

    if let Some(p) = reject {
        let mut w = BufWriter::new(
            File::create(p).with_context(|| format!("failed to create {}", p.display()))?,
        );
        collector.write_reject(&mut w)?;
        w.flush()?;
    }

    // 統計は stderr へ（標準出力の辞書をパイプで汚さない）。
    eprintln!(
        "{}",
        msg!(
            format!(
                "done: {} danger word(s), {} rejected pair(s).",
                collector.danger_len(),
                collector.reject_pairs().len()
            ),
            format!(
                "完了: 危険語 {} 語、除外ペア {} 件。",
                collector.danger_len(),
                collector.reject_pairs().len()
            ),
        )
    );
    Ok(())
}

/// 危険語リストを output（なければ標準出力）へ書き出す。
fn emit_dict(collector: &Collector, output: Option<&Path>, spec: OutputSpec) -> Result<()> {
    match output {
        Some(p) => {
            let mut w = BufWriter::new(
                File::create(p).with_context(|| format!("failed to create {}", p.display()))?,
            );
            write_body(collector, &mut w, spec)?;
            w.flush()?;
        }
        None => {
            let stdout = io::stdout();
            let mut w = stdout.lock();
            write_body(collector, &mut w, spec)?;
            w.flush()?;
        }
    }
    Ok(())
}

fn write_body(collector: &Collector, w: &mut impl Write, spec: OutputSpec) -> io::Result<()> {
    match spec.format {
        OutputFormat::Txt => collector.write_dict(w),
        OutputFormat::Counts => collector.write_dict_with_counts(w),
        OutputFormat::AmicalJson => collector.write_amical_json(w, spec.field),
    }
}

/// 対話ループ。正解文→認識結果を交互に受け取り、即時 diff して蓄積する。
fn run_repl(
    tokenizer: &LinderaTokenizer,
    opts: &NormalizeOptions,
    output: Option<&Path>,
    reject: Option<&Path>,
    spec: OutputSpec,
) -> Result<()> {
    let stdin = io::stdin();
    let mut lines = stdin.lock().lines();
    let mut collector = Collector::new();

    eprintln!(
        "{}",
        msg!(
            "Interactive mode. Enter a reference sentence, then its ASR result. Empty line or Ctrl-D to finish.",
            "対話モード。正解文 → その認識結果の順に入力。空行か Ctrl-D で終了。",
        )
    );

    loop {
        eprint!("{}", msg!("reference> ", "正解文> "));
        io::stderr().flush().ok();
        let reference = match lines.next() {
            Some(line) => line?,
            None => break,
        };
        if reference.trim().is_empty() {
            break;
        }

        eprint!("{}", msg!("asr result> ", "認識結果> "));
        io::stderr().flush().ok();
        let hypothesis = match lines.next() {
            Some(line) => line?,
            None => break,
        };

        let outs = process(tokenizer, &reference, &hypothesis, opts)?;
        // 即時表示：[+] 採用（同音）／[-] 除外（読み違い）。
        for o in &outs {
            let c = o.candidate();
            if o.is_homophone() {
                eprintln!(
                    "  [+] {} \u{2190} {} ({})",
                    c.reference_surface, c.hypothesis_surface, c.reference_reading
                );
            } else {
                eprintln!(
                    "  [-] {} / {} ({} / {})",
                    c.reference_surface,
                    c.hypothesis_surface,
                    c.reference_reading,
                    c.hypothesis_reading
                );
            }
        }
        collector.add_all(outs);

        // 途中保存（中断しても残す）。選択された形式で上書き保存する。
        if let Some(p) = output {
            let mut w = BufWriter::new(File::create(p)?);
            write_body(&collector, &mut w, spec)?;
            w.flush()?;
        }
        if let Some(p) = reject {
            let mut w = BufWriter::new(File::create(p)?);
            collector.write_reject(&mut w)?;
            w.flush()?;
        }
    }

    eprintln!();
    eprintln!(
        "{}",
        msg!(
            format!("collected {} danger word(s).", collector.danger_len()),
            format!("危険語 {} 語を収集しました。", collector.danger_len()),
        )
    );
    // output 未指定なら最後に標準出力へ出す。
    if output.is_none() {
        let stdout = io::stdout();
        let mut w = stdout.lock();
        write_body(&collector, &mut w, spec)?;
        w.flush()?;
    }
    Ok(())
}

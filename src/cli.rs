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
        /// 出力に出現回数を付ける（`語\t回数`）。
        #[arg(long)]
        counts: bool,
    },
    /// 対話ループ：正解文→認識結果を貼り、即時 diff して蓄積する。
    Repl {
        /// 危険語リストの保存先（ペア処理ごとに上書き保存）。
        #[arg(long, short)]
        output: Option<PathBuf>,
        /// 読み不一致ペアの別ログ保存先。
        #[arg(long)]
        reject: Option<PathBuf>,
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
            counts,
        } => run_batch(
            &tokenizer,
            &opts,
            &reference,
            &hypothesis,
            output.as_deref(),
            reject.as_deref(),
            counts,
        ),
        Command::Repl { output, reject } => {
            run_repl(&tokenizer, &opts, output.as_deref(), reject.as_deref())
        }
    }
}

/// 正解文・認識文ファイルを行対応で照合する。
fn run_batch(
    tokenizer: &LinderaTokenizer,
    opts: &NormalizeOptions,
    reference: &Path,
    hypothesis: &Path,
    output: Option<&Path>,
    reject: Option<&Path>,
    counts: bool,
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

    emit_dict(&collector, output, counts)?;

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
fn emit_dict(collector: &Collector, output: Option<&Path>, counts: bool) -> Result<()> {
    match output {
        Some(p) => {
            let mut w = BufWriter::new(
                File::create(p).with_context(|| format!("failed to create {}", p.display()))?,
            );
            write_body(collector, &mut w, counts)?;
            w.flush()?;
        }
        None => {
            let stdout = io::stdout();
            let mut w = stdout.lock();
            write_body(collector, &mut w, counts)?;
            w.flush()?;
        }
    }
    Ok(())
}

fn write_body(collector: &Collector, w: &mut impl Write, counts: bool) -> io::Result<()> {
    if counts {
        collector.write_dict_with_counts(w)
    } else {
        collector.write_dict(w)
    }
}

/// 対話ループ。正解文→認識結果を交互に受け取り、即時 diff して蓄積する。
fn run_repl(
    tokenizer: &LinderaTokenizer,
    opts: &NormalizeOptions,
    output: Option<&Path>,
    reject: Option<&Path>,
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

        // 途中保存（中断しても残す）。
        if let Some(p) = output {
            let mut w = BufWriter::new(File::create(p)?);
            collector.write_dict(&mut w)?;
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
        collector.write_dict(&mut w)?;
        w.flush()?;
    }
    Ok(())
}

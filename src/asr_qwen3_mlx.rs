//! Qwen3-ASR（mlx-audio）による音声認識アダプタ（SPEC 9 章）。
//!
//! Step 1 は素朴な「1 ファイル = 1 プロセス」版。モデルロードが毎回走るため
//! 1 文あたり約 2 秒かかる（Step 0 実測）。遅さが効いてきた段階で SPEC 9 章の
//! バッチドライバ（`scripts/qwen3_asr_batch.py`・モデル 1 回ロード・JSONL 会話)
//! に置き換える。
//!
//! mlx-audio 0.4.4 の CLI 形状（Step 0 実測・`--help` で確認済み）:
//! - `--output-path` は必須。`{output-path}.txt` にプレーンテキストを書く。
//! - `--context` が hotwords（コンテキストバイアシング）の入口。
//!   区切りは半角スペースの初期仮定（SPEC Q1。evaluate 構築前に公式実装で確定）。

use crate::recognize::Recognizer;
use anyhow::{bail, Context, Result};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

pub struct Qwen3MlxRecognizer {
    /// mlx-audio が入った環境の python。venv を activate して biasdiff を
    /// 起動すれば既定の "python3" がそれを指す。
    pub python: PathBuf,
    /// モデル名（例: `mlx-community/Qwen3-ASR-0.6B-8bit`）。
    pub model: String,
}

impl Qwen3MlxRecognizer {
    pub fn new(python: impl Into<PathBuf>, model: impl Into<String>) -> Self {
        Self {
            python: python.into(),
            model: model.into(),
        }
    }
}

impl Recognizer for Qwen3MlxRecognizer {
    fn recognize(&self, audio: &Path, bias: Option<&[String]>) -> Result<String> {
        // 出力ベース。mlx-audio はここに ".txt" を足したファイルを書く。
        let stem = audio
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "audio".to_string());
        let out_base = std::env::temp_dir().join(format!(
            "biasdiff-asr-{}-{}",
            std::process::id(),
            stem
        ));
        let txt = PathBuf::from(format!("{}.txt", out_base.display()));

        let mut cmd = Command::new(&self.python);
        cmd.args(["-m", "mlx_audio.stt.generate", "--model", &self.model, "--audio"])
            .arg(audio)
            .arg("--output-path")
            .arg(&out_base);
        if let Some(words) = bias {
            if !words.is_empty() {
                cmd.args(["--context", &words.join(" ")]);
            }
        }

        let output = cmd.output().with_context(|| {
            format!(
                "failed to run {} -m mlx_audio.stt.generate (venv active?)",
                self.python.display()
            )
        })?;
        if !output.status.success() {
            // stderr は進捗バーを含み長いので、末尾だけ要約して返す。
            let stderr = String::from_utf8_lossy(&output.stderr);
            let tail: Vec<&str> = stderr.lines().rev().take(5).collect();
            let tail: Vec<&str> = tail.into_iter().rev().collect();
            bail!(
                "mlx_audio.stt.generate failed for {} (model {}): {}",
                audio.display(),
                self.model,
                tail.join(" | ")
            );
        }

        let text = fs::read_to_string(&txt).with_context(|| {
            format!(
                "ASR output {} not found (mlx-audio CLI shape changed? expected --output-path + .txt)",
                txt.display()
            )
        })?;
        let _ = fs::remove_file(&txt);
        Ok(text.trim().to_string())
    }
}

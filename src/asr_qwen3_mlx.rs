//! Qwen3-ASR（mlx-audio）による音声認識アダプタ（SPEC 9 章）。
//!
//! 同梱のバッチドライバ（`scripts/qwen3_asr_batch.py`、コンパイル時に
//! 埋め込み・実行時に temp へ実体化）とJSONL で会話する。ドライバ方式で
//! ある理由は 2 つ:
//!
//! 1. **速度** — モデルロードがプロセスコストの大半（Step 0 実測で 1 文
//!    約 2 秒）。1 回ロードして会話し続ければ長い収穫が現実的になる。
//! 2. **バイアシングの唯一の経路** — mlx-audio 0.4.4 の CLI は `--context`
//!    を受けるが、kwargs を `inspect.signature(model.generate)` で濾すため
//!    Qwen3 の generate に無い `context` は**黙って捨てられる**（導入済み
//!    ソースを読んで確認）。効くのは Python API の `system_prompt=` だけで、
//!    それにはドライバが要る。
//!
//! `bias` の書式は半角スペース区切りの語の羅列・前置きなし（公式 Qwen3-ASR の
//! `context` 例と同じ。SPEC Q1 で確定）。

use crate::recognize::Recognizer;
use anyhow::{bail, Context, Result};
use std::cell::RefCell;
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

/// コンパイル時に埋め込むドライバ本体。バイナリを自己完結に保つ（D2）。
const DRIVER_SOURCE: &str = include_str!("../scripts/qwen3_asr_batch.py");

pub struct Qwen3MlxRecognizer {
    /// mlx-audio が入った環境の python。venv を activate して biasdiff を
    /// 起動すれば既定の "python3" がそれを指す。
    python: PathBuf,
    /// モデル名（例: `mlx-community/Qwen3-ASR-0.6B-8bit`）。
    model: String,
    /// 遅延起動のドライバ。最初の認識要求でモデルを 1 回だけロードする。
    driver: RefCell<Option<Driver>>,
}

struct Driver {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    /// 実体化したドライバスクリプト（Drop で掃除）。
    script_path: PathBuf,
    /// リクエスト連番。応答の取り違えをプロトコル違反として検出する。
    next_id: u64,
}

impl Qwen3MlxRecognizer {
    pub fn new(python: impl Into<PathBuf>, model: impl Into<String>) -> Self {
        Self {
            python: python.into(),
            model: model.into(),
            driver: RefCell::new(None),
        }
    }

    /// ドライバを（必要なら）起動し、ready ハンドシェイクまで待つ。
    fn ensure_driver(&self) -> Result<()> {
        let mut slot = self.driver.borrow_mut();
        if slot.is_some() {
            return Ok(());
        }

        let script_path = std::env::temp_dir().join(format!(
            "biasdiff-qwen3-driver-{}.py",
            std::process::id()
        ));
        fs::write(&script_path, DRIVER_SOURCE)
            .with_context(|| format!("failed to write {}", script_path.display()))?;

        // stderr は親に流す: 初回のモデルダウンロード進捗をユーザーから隠さない。
        let mut child = Command::new(&self.python)
            .arg("-u")
            .arg(&script_path)
            .args(["--model", &self.model])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .with_context(|| {
                format!(
                    "failed to start ASR driver via {} (mlx-audio venv active?)",
                    self.python.display()
                )
            })?;
        let stdin = child.stdin.take().expect("stdin was piped");
        let stdout = child.stdout.take().expect("stdout was piped");
        let mut stdout = BufReader::new(stdout);

        // ready 行（モデルロード完了）を待つ。EOF はドライバ即死を意味する。
        let mut line = String::new();
        stdout
            .read_line(&mut line)
            .context("failed to read from ASR driver")?;
        if line.is_empty() {
            bail!(
                "ASR driver exited before becoming ready (model {}; is mlx-audio installed in {}?)",
                self.model,
                self.python.display()
            );
        }
        let ready: serde_json::Value = serde_json::from_str(line.trim())
            .with_context(|| format!("ASR driver sent non-JSON instead of ready: {line:?}"))?;
        if ready.get("ready").and_then(|v| v.as_bool()) != Some(true) {
            bail!("ASR driver handshake failed: {line:?}");
        }

        *slot = Some(Driver {
            child,
            stdin,
            stdout,
            script_path,
            next_id: 0,
        });
        Ok(())
    }
}

impl Recognizer for Qwen3MlxRecognizer {
    fn recognize(&self, audio: &Path, bias: Option<&[String]>) -> Result<String> {
        self.ensure_driver()?;
        let mut slot = self.driver.borrow_mut();
        let driver = slot.as_mut().expect("ensure_driver just filled this");

        driver.next_id += 1;
        let id = driver.next_id.to_string();
        let request = serde_json::json!({
            "id": id,
            "audio": audio.to_string_lossy(),
            "bias": bias,
        });
        writeln!(driver.stdin, "{request}").context("failed to write to ASR driver")?;

        let mut line = String::new();
        driver
            .stdout
            .read_line(&mut line)
            .context("failed to read from ASR driver")?;
        if line.is_empty() {
            bail!("ASR driver exited mid-conversation (audio {})", audio.display());
        }
        let resp: serde_json::Value = serde_json::from_str(line.trim())
            .with_context(|| format!("ASR driver sent non-JSON: {line:?}"))?;
        if resp.get("id").and_then(|v| v.as_str()) != Some(id.as_str()) {
            bail!("ASR driver protocol violation: response id mismatch ({line:?})");
        }
        if resp.get("ok").and_then(|v| v.as_bool()) != Some(true) {
            bail!(
                "ASR failed for {}: {}",
                audio.display(),
                resp.get("error").and_then(|v| v.as_str()).unwrap_or("unknown error")
            );
        }
        let text = resp
            .get("text")
            .and_then(|v| v.as_str())
            .context("ASR driver response missing text")?;
        Ok(text.trim().to_string())
    }
}

impl Drop for Qwen3MlxRecognizer {
    fn drop(&mut self) {
        if let Some(mut driver) = self.driver.borrow_mut().take() {
            // stdin を閉じれば for-line ループが終わり、ドライバは自然終了する。
            drop(driver.stdin);
            let _ = driver.child.wait();
            let _ = fs::remove_file(&driver.script_path);
        }
    }
}

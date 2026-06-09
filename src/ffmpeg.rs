//! ffmpeg による 16 kHz mono WAV への正規化。
//!
//! 認識層の入力契約（SPEC 8 章）。TTS アダプタ（VOICEVOX / say）が共通で使う。
//! ffmpeg は PATH 上の実行ファイルを subprocess で呼ぶ — コンパイル時依存にしない。

use anyhow::{bail, Context, Result};
use std::path::Path;
use std::process::Command;

/// `input` の音声を 16 kHz mono WAV へ変換して `output` に書く。
///
/// `-f wav` を明示する: 出力先は書き込み途中を示す一時名（`.part`）のことが
/// あり、ffmpeg の拡張子推定に任せると「フォーマットを選べない」で落ちる。
pub fn normalize_to_16k_mono(input: &Path, output: &Path) -> Result<()> {
    let out = Command::new("ffmpeg")
        .args(["-y", "-loglevel", "error", "-i"])
        .arg(input)
        .args(["-ar", "16000", "-ac", "1", "-f", "wav"])
        .arg(output)
        .output()
        .context("failed to run ffmpeg (is it installed and on PATH?)")?;
    if !out.status.success() {
        // stderr は複数行になりがちなので、実のある最終行だけ要約する。
        let stderr = String::from_utf8_lossy(&out.stderr);
        let last = stderr
            .lines()
            .rev()
            .find(|l| !l.trim().is_empty())
            .unwrap_or("");
        bail!("ffmpeg failed for {}: {}", input.display(), last);
    }
    Ok(())
}

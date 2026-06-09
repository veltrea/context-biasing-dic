//! macOS `say` による音声合成アダプタ（SPEC 8 章のフォールバック）。
//!
//! 追加導入ゼロで動く代わりに、話速の指定が words-per-minute（語/分）で
//! 倍率と直接対応しない。ここでは日本語のおおよその既定 180 wpm を基準に
//! 倍率を掛ける近似で扱う（rate 1.0 のときは -r を渡さずエンジン既定に任せる）。

use crate::ffmpeg::normalize_to_16k_mono;
use crate::synth::{Synthesizer, VoiceSpec};
use anyhow::{bail, Context, Result};
use std::fs;
use std::path::Path;
use std::process::Command;

/// rate 倍率 → wpm 換算の基準値（日本語 `say` の体感既定）。
const BASE_WPM: f32 = 180.0;

pub struct SaySynth;

impl Synthesizer for SaySynth {
    fn synth(&self, text: &str, voice: &VoiceSpec, out: &Path) -> Result<()> {
        let aiff = out.with_file_name(format!(
            "{}.raw.aiff",
            out.file_name().unwrap_or_default().to_string_lossy()
        ));

        let mut cmd = Command::new("say");
        cmd.args(["-v", &voice.voice, "-o"]).arg(&aiff);
        if (voice.rate - 1.0).abs() > f32::EPSILON {
            cmd.args(["-r", &format!("{}", (BASE_WPM * voice.rate).round() as u32)]);
        }
        // テキストは引数で渡す（Command はシェルを介さないのでエスケープ不要）。
        cmd.arg(text);

        let output = cmd.output().context("failed to run `say` (macOS only)")?;
        if !output.status.success() {
            bail!(
                "`say` failed (voice {}): {}",
                voice.voice,
                String::from_utf8_lossy(&output.stderr).trim()
            );
        }

        let result = normalize_to_16k_mono(&aiff, out);
        let _ = fs::remove_file(&aiff);
        result
    }
}

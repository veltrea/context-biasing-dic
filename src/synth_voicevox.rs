//! VOICEVOX による音声合成アダプタ（SPEC 8 章）。
//!
//! ローカル HTTP エンジン（既定 `127.0.0.1:50021`）を 2 段で叩く:
//! `/audio_query`（合成パラメータ JSON）→ `/synthesis`（WAV バイト列）。
//! 話速は audio_query JSON の `speedScale` で指定する。得られた WAV
//! （24 kHz）は ffmpeg で 16 kHz mono に正規化して出力契約を満たす。

use crate::ffmpeg::normalize_to_16k_mono;
use crate::synth::{Synthesizer, VoiceSpec};
use anyhow::{bail, Context, Result};
use std::fs;
use std::io::Read;
use std::path::Path;

pub struct VoicevoxSynth {
    base: String,
}

impl VoicevoxSynth {
    /// `base` は `http://127.0.0.1:50021` の形（末尾スラッシュなし）。
    pub fn new(base: impl Into<String>) -> Self {
        Self { base: base.into() }
    }

    /// エンジンの疎通確認。起動直後に呼んで、つながらないなら分かりやすく
    /// 失敗させる（文を処理し始めてから死ぬより親切）。
    pub fn check(&self) -> Result<String> {
        let version = ureq::get(&format!("{}/version", self.base))
            .call()
            .with_context(|| format!("VOICEVOX engine not reachable at {} (is it running?)", self.base))?
            .into_string()?;
        Ok(version.trim().trim_matches('"').to_string())
    }
}

impl Synthesizer for VoicevoxSynth {
    fn synth(&self, text: &str, voice: &VoiceSpec, out: &Path) -> Result<()> {
        // 1. 合成パラメータを得る（text はクエリパラメータとして URL エンコードされる）。
        let query_json = ureq::post(&format!("{}/audio_query", self.base))
            .query("speaker", &voice.voice)
            .query("text", text)
            .call()
            .with_context(|| {
                format!(
                    "VOICEVOX audio_query failed (speaker {}; engine at {})",
                    voice.voice, self.base
                )
            })?
            .into_string()?;

        // 2. 話速を埋める。既定 1.0 でも明示して、エンジン側の既定変更に揺れない。
        let mut query: serde_json::Value =
            serde_json::from_str(&query_json).context("VOICEVOX audio_query returned non-JSON")?;
        query["speedScale"] = serde_json::json!(voice.rate);

        // 3. WAV を得る。
        let resp = ureq::post(&format!("{}/synthesis", self.base))
            .query("speaker", &voice.voice)
            .set("Content-Type", "application/json")
            .send_string(&query.to_string())
            .with_context(|| format!("VOICEVOX synthesis failed (speaker {})", voice.voice))?;
        let mut wav = Vec::new();
        resp.into_reader()
            .read_to_end(&mut wav)
            .context("failed to read VOICEVOX synthesis response")?;
        if wav.len() < 44 {
            // WAV ヘッダにも満たない応答は何かがおかしい。
            bail!("VOICEVOX returned suspiciously small audio ({} bytes)", wav.len());
        }

        // 4. 16 kHz mono へ正規化（認識層の入力契約）。生 WAV は out の隣に
        //    一時置きして消す。
        let raw = out.with_file_name(format!(
            "{}.raw.wav",
            out.file_name().unwrap_or_default().to_string_lossy()
        ));
        fs::write(&raw, &wav).with_context(|| format!("failed to write {}", raw.display()))?;
        let result = normalize_to_16k_mono(&raw, out);
        let _ = fs::remove_file(&raw);
        result
    }
}

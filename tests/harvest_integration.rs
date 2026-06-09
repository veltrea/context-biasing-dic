//! アダプタの統合テスト（`harvest` feature 限定・実エンジン必要）。
//!
//! すべて `#[ignore]`: VOICEVOX の稼働と mlx-audio 入りの venv が前提のため、
//! CI や通常の `cargo test` では走らせない。手動実行:
//!
//! ```sh
//! source ~/.venvs/mlx-audio/bin/activate
//! cargo test --features harvest -- --ignored
//! ```
//!
//! python の場所は環境変数 `BIASDIFF_ASR_PYTHON` で上書きできる（既定 python3 =
//! activate 済みの venv を想定)。

#![cfg(feature = "harvest")]

use biasdiff::asr_qwen3_mlx::Qwen3MlxRecognizer;
use biasdiff::recognize::Recognizer;
use biasdiff::synth::{Synthesizer, VoiceSpec};
use biasdiff::synth_voicevox::VoicevoxSynth;
use std::fs;
use std::path::PathBuf;

const VOICEVOX_URL: &str = "http://127.0.0.1:50021";
const MODEL: &str = "mlx-community/Qwen3-ASR-0.6B-8bit";

fn tmp(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("biasdiff-itest-{}-{}", std::process::id(), name))
}

fn asr_python() -> PathBuf {
    std::env::var("BIASDIFF_ASR_PYTHON")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("python3"))
}

/// VOICEVOX 合成 → 16 kHz mono WAV が出力契約どおり書かれる。
#[test]
#[ignore]
fn voicevox_synthesizes_16k_mono_wav() {
    let synth = VoicevoxSynth::new(VOICEVOX_URL);
    synth.check().expect("VOICEVOX engine must be running");

    let out = tmp("vv.wav");
    let voice = VoiceSpec {
        engine: "voicevox".into(),
        voice: "3".into(),
        rate: 1.0,
    };
    synth.synth("疎通確認のための文です", &voice, &out).unwrap();

    let bytes = fs::read(&out).unwrap();
    assert!(bytes.len() > 44, "wav should have content");
    assert_eq!(&bytes[0..4], b"RIFF");
    // fmt チャンクのサンプルレート（オフセット 24..28, LE）が 16000。
    let rate = u32::from_le_bytes([bytes[24], bytes[25], bytes[26], bytes[27]]);
    assert_eq!(rate, 16000);
    // チャンネル数（オフセット 22..24, LE）が 1。
    let ch = u16::from_le_bytes([bytes[22], bytes[23]]);
    assert_eq!(ch, 1);
    let _ = fs::remove_file(&out);
}

/// 合成 → 認識の往復。認識文字列が空でなく日本語を含む。
#[test]
#[ignore]
fn qwen3_recognizes_voicevox_audio() {
    let synth = VoicevoxSynth::new(VOICEVOX_URL);
    synth.check().expect("VOICEVOX engine must be running");

    let out = tmp("rt.wav");
    let voice = VoiceSpec {
        engine: "voicevox".into(),
        voice: "3".into(),
        rate: 1.0,
    };
    synth.synth("機械学習で意思決定を支援する", &voice, &out).unwrap();

    let rec = Qwen3MlxRecognizer::new(asr_python(), MODEL);
    let text = rec.recognize(&out, None).unwrap();
    assert!(!text.is_empty());
    assert!(
        text.chars().any(|c| ('\u{3040}'..='\u{9FFF}').contains(&c)),
        "expected Japanese text, got: {text}"
    );
    let _ = fs::remove_file(&out);
}

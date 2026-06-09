//! 音声認識の抽象（SPEC 5 章）。
//!
//! `bias` はコンテキストバイアシングの語リスト。`None` は素の認識。
//! Qwen3-ASR では system プロンプト（mlx-audio CLI の `--context`）に対応する。
//! `evaluate` が辞書の効果を測るときに使う口で、`harvest` は `None` で呼ぶ。

use std::path::Path;

/// 音声ファイルを文字起こしする。
pub trait Recognizer {
    fn recognize(&self, audio: &Path, bias: Option<&[String]>) -> anyhow::Result<String>;
}

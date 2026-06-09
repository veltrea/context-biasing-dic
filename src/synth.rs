//! 音声合成の抽象（SPEC 5 章）。
//!
//! 1 文を 16 kHz mono WAV に合成して `out` へ書くことが契約のすべて。
//! 出力先のパス（内容アドレスのキャッシュ位置）はオーケストレータが決めて
//! 渡す — キャッシュの知識をアダプタに持ち込まないため。

use std::path::Path;

/// 合成 1 構成（エンジンの声 + 話速）。声マトリクスの 1 セル。
#[derive(Debug, Clone, PartialEq)]
pub struct VoiceSpec {
    /// エンジン名。"voicevox" | "say"。
    pub engine: String,
    /// 話者。VOICEVOX は speaker id（数値文字列）、say は声名（Kyoko など）。
    pub voice: String,
    /// 話速倍率。1.0 = エンジン既定。
    pub rate: f32,
}

impl VoiceSpec {
    /// 投票（vote）で「同一話者」とみなす単位。話速違いは同じ話者に数える
    /// （SPEC 11 章: 採用条件は「異なる話者」の数）。
    pub fn speaker_key(&self) -> String {
        format!("{}:{}", self.engine, self.voice)
    }

    /// キャッシュキーに使う rate の安定した文字列表現。
    /// f32 の Display は 1.0 を "1" にするため、桁数を固定して揺れを断つ。
    pub fn rate_key(&self) -> String {
        format!("{:.3}", self.rate)
    }
}

/// 1 文を音声ファイルに合成する。出力契約は 16 kHz mono WAV（認識層の入力契約）。
pub trait Synthesizer {
    fn synth(&self, text: &str, voice: &VoiceSpec, out: &Path) -> anyhow::Result<()>;
}

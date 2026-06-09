//! コンテキストバイアシング辞書構築のための diff 照合ユーティリティ。
//!
//! 正解文（コピペで用意した例文）と ASR の認識結果を形態素単位で diff し、
//! 置換ペアのうち「読みが一致するもの（＝同音衝突）」だけを危険語候補として集める。
//!
//! 設計の核心は層分けにある。`token` / `reading` / `diff` / `pipeline` / `collect`
//! は形態素解析器の実体に依存しない純粋ロジックで、単体テストできる。
//! 形態素解析・読み付与（Lindera）は `morph` に閉じ込め、`token::Tokenizer`
//! トレイト越しに注入する。これによりコアは辞書のダウンロードもバイナリも不要で検証できる。

pub mod collect;
pub mod diff;
pub mod pipeline;
pub mod reading;
pub mod token;

// v0.2 harvest の抽象（純粋: 型とトレイトのみ、std で単体テスト可能）。
// 実体アダプタは `harvest` feature の向こう側に置く（SPEC 4 章）。
pub mod recognize;
pub mod source;
pub mod synth;
pub mod vote;

// Lindera バックエンドと UI（feature gated）。辞書を埋め込んだときだけコンパイルされる。
#[cfg(feature = "_lindera")]
pub mod cli;
#[cfg(feature = "_lindera")]
pub mod messages;
#[cfg(feature = "_lindera")]
pub mod morph;

// v0.2 harvest のオーケストレーションとアダプタ（feature gated）。
// 外部エンジン（VOICEVOX / say / ffmpeg / Qwen3-ASR）へはここからだけ触れる。
#[cfg(feature = "harvest")]
pub mod asr_qwen3_mlx;
#[cfg(feature = "harvest")]
pub mod ffmpeg;
#[cfg(feature = "harvest")]
pub mod harvest;
#[cfg(feature = "harvest")]
pub mod source_file;
#[cfg(feature = "harvest")]
pub mod synth_say;
#[cfg(feature = "harvest")]
pub mod synth_voicevox;

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

// Lindera バックエンドと UI（feature gated）。辞書を埋め込んだときだけコンパイルされる。
#[cfg(feature = "_lindera")]
pub mod cli;
#[cfg(feature = "_lindera")]
pub mod messages;
#[cfg(feature = "_lindera")]
pub mod morph;

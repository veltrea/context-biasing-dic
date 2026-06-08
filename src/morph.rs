//! Lindera による形態素解析・読み付与の実装。
//!
//! このモジュールだけが Lindera に依存する。`token::Tokenizer` トレイトを実装し、
//! コア層へは抽象越しに渡る。辞書はビルド時に埋め込む（`embedded://...`）。
//!
//! 読み（カタカナ）の位置は辞書フォーマットで異なる:
//!   - IPADIC: details[7] = 読み
//!   - UniDic: details[9] = 発音形出現形（実発音に近い）
//! 取得できない（未知語・記号で `*` や欠落）場合は表記のカタカナ化にフォールバックする。

use crate::token::{Token, Tokenizer};
use anyhow::{Context, Result};
use lindera::dictionary::load_dictionary;
use lindera::mode::Mode;
use lindera::segmenter::Segmenter;
use lindera::tokenizer::Tokenizer as LinderaCore;

/// 埋め込み辞書の種別。読みを取り出す details インデックスが異なる。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DictKind {
    Ipadic,
    Unidic,
}

impl DictKind {
    /// 読み（カタカナ）が入る details のインデックス。
    fn reading_index(&self) -> usize {
        match self {
            DictKind::Ipadic => 7, // 読み
            DictKind::Unidic => 9, // 発音形出現形
        }
    }

    /// `load_dictionary` へ渡す URI。
    fn uri(&self) -> &'static str {
        match self {
            DictKind::Ipadic => "embedded://ipadic",
            DictKind::Unidic => "embedded://unidic",
        }
    }
}

/// Lindera を内部に持つ形態素解析器。
pub struct LinderaTokenizer {
    inner: LinderaCore,
    kind: DictKind,
}

impl LinderaTokenizer {
    /// 指定した種別の埋め込み辞書でトークナイザを構築する。
    ///
    /// その辞書がビルドに埋め込まれていない場合は読み込みに失敗する。
    pub fn new(kind: DictKind) -> Result<Self> {
        let dictionary = load_dictionary(kind.uri()).with_context(|| {
            format!(
                "failed to load dictionary {} (was it embedded at build time? try --features ipadic|unidic)",
                kind.uri()
            )
        })?;
        let segmenter = Segmenter::new(Mode::Normal, dictionary, None);
        let inner = LinderaCore::new(segmenter);
        Ok(Self { inner, kind })
    }
}

impl Tokenizer for LinderaTokenizer {
    fn tokenize(&self, text: &str) -> Result<Vec<Token>> {
        let mut tokens = self.inner.tokenize(text)?;
        let idx = self.kind.reading_index();

        let mut out = Vec::with_capacity(tokens.len());
        for token in tokens.iter_mut() {
            let surface = token.surface.to_string();
            let details = token.details();
            let pos = details
                .first()
                .map(|s| s.to_string())
                .unwrap_or_else(|| "*".to_string());
            let reading = details
                .get(idx)
                .map(|s| s.to_string())
                .filter(|s| !s.is_empty() && s != "*")
                .unwrap_or_else(|| crate::reading::to_katakana(&surface));
            out.push(Token {
                surface,
                reading,
                pos,
            });
        }
        Ok(out)
    }
}

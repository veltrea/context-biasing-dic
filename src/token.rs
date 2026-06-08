//! トークン（形態素）の表現と、形態素解析器の抽象。
//!
//! コア層はこの `Tokenizer` トレイトにのみ依存する。実体（Lindera）でも
//! テスト用モックでも差し替えられるようにして、テスト容易性を担保する。

/// 形態素1つ分。表記・読み・品詞大分類を持つ。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Token {
    /// 表記（書字形出現形）。例: "形態"
    pub surface: String,
    /// 読み（カタカナ）。取得できなければ表記のカタカナ化にフォールバックする。
    pub reading: String,
    /// 品詞大分類。例: "名詞"。挿入/削除の傾向分析などに使えるよう保持する。
    pub pos: String,
}

impl Token {
    /// テストや手組みのための簡易コンストラクタ。
    pub fn new(
        surface: impl Into<String>,
        reading: impl Into<String>,
        pos: impl Into<String>,
    ) -> Self {
        Self {
            surface: surface.into(),
            reading: reading.into(),
            pos: pos.into(),
        }
    }
}

/// 形態素解析器の抽象。
///
/// 与えたテキストをトークン列へ分解し、各トークンに読みを付与する責務を負う。
/// コア層（diff・読みフィルタ）はこのトレイト越しにのみ解析器へ触れる。
pub trait Tokenizer {
    fn tokenize(&self, text: &str) -> anyhow::Result<Vec<Token>>;
}

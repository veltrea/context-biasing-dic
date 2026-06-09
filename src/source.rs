//! テキストソースの抽象（SPEC 5 章）。
//!
//! 記事の取得だけを責務とし、本文のパース（例文化）は持たない。実装は薄く保ち、
//! Qiita / Zenn / ローカルファイルのアダプタが `harvest` feature の向こうで
//! このトレイトを実装する。型とトレイトは純粋（std のみ）で、モックを書ける。

/// 取得した記事 1 本。本文の形式はソースが宣言する。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Article {
    /// ソース内でのキャッシュ・重複排除キー。
    pub id: String,
    /// 記事タイトル。
    pub title: String,
    /// 記事 URL（ローカルファイルでは空でよい）。
    pub url: String,
    /// 人気度。Qiita: stocks_count / Zenn: liked_count / ファイル: 0。
    pub popularity: u32,
    /// 本文。
    pub body: Body,
}

/// 本文の形式。例文化（extract）がフロントエンドを選ぶ手がかり。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Body {
    /// Qiita API の本文（生 Markdown）。
    Markdown(String),
    /// Zenn API の本文（HTML）。
    Html(String),
    /// ローカルファイルなどのプレーンテキスト。
    Plain(String),
}

/// 候補記事を取得する。`n` はソースごとの記事数上限。
pub trait TextSource {
    fn fetch(&self, n: usize) -> anyhow::Result<Vec<Article>>;
}

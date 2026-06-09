//! Qiita 公式 API v2 のテキストソース（SPEC 6 章）。
//!
//! 一覧エンドポイントが本文（生 Markdown）込みで最大 100 件を返すため、
//! 1 回の実行で叩く API は 1 リクエストだけ。取得した記事は
//! `articles/qiita/{id}.json` に保存する（実行間の参照・監査用。一覧は
//! 検索結果の鮮度が大事なので毎回叩く — 新着を拾うのが日次増分の目的）。
//!
//! `QIITA_TOKEN`（任意）はレート上限を 60 → 1000 req/h に引き上げる。
//! qiita.com にのみ送られる。

use crate::source::{Article, Body, TextSource};
use anyhow::{Context, Result};
use std::fs;
use std::path::PathBuf;

/// User-Agent。非ブラウザのツールとして名乗る（Qiita/Zenn 共通の流儀）。
pub const USER_AGENT: &str = concat!(
    "biasdiff/",
    env!("CARGO_PKG_VERSION"),
    " (+https://github.com/veltrea/context-biasing-dic)"
);

pub struct QiitaSource {
    /// Qiita の検索クエリ（例: `stocks:>=50 tag:rust`）。
    pub query: String,
    /// `QIITA_TOKEN`。CLI が環境変数から読んで渡す。
    pub token: Option<String>,
    /// キャッシュルート（`articles/qiita/` を下に作る）。
    pub cache_dir: PathBuf,
}

impl TextSource for QiitaSource {
    fn fetch(&self, n: usize) -> Result<Vec<Article>> {
        // per_page の上限は 100。それ以上はページングが要るが、1 日 100 記事で
        // 十分（収穫は文数上限の方が先に効く）。
        let per_page = n.clamp(1, 100).to_string();
        let mut req = ureq::get("https://qiita.com/api/v2/items")
            .query("per_page", &per_page)
            .query("query", &self.query)
            .set("User-Agent", USER_AGENT);
        if let Some(t) = &self.token {
            req = req.set("Authorization", &format!("Bearer {t}"));
        }
        let body = req
            .call()
            .context("Qiita API request failed (rate limit? network?)")?
            .into_string()
            .context("failed to read Qiita API response")?;
        let items: serde_json::Value =
            serde_json::from_str(&body).context("Qiita API returned non-JSON")?;
        let Some(items) = items.as_array() else {
            anyhow::bail!("Qiita API returned unexpected shape (expected an array)");
        };

        let article_dir = self.cache_dir.join("articles").join("qiita");
        fs::create_dir_all(&article_dir)
            .with_context(|| format!("failed to create {}", article_dir.display()))?;

        let mut out = Vec::new();
        for item in items.iter().take(n) {
            let id = str_of(item, "id");
            if id.is_empty() {
                continue;
            }
            // 記事 JSON を保存（再実行の参照・監査用）。失敗しても収穫は続ける。
            let path = article_dir.join(format!("{id}.json"));
            if !path.exists() {
                if let Ok(s) = serde_json::to_string(item) {
                    let _ = fs::write(&path, s);
                }
            }
            out.push(Article {
                id,
                title: str_of(item, "title"),
                url: str_of(item, "url"),
                popularity: u32_of(item, "stocks_count").max(u32_of(item, "likes_count")),
                body: Body::Markdown(str_of(item, "body")),
            });
        }
        Ok(out)
    }

    fn label(&self) -> &str {
        "qiita"
    }
}

fn str_of(v: &serde_json::Value, key: &str) -> String {
    v.get(key)
        .and_then(|x| x.as_str())
        .unwrap_or_default()
        .to_string()
}

fn u32_of(v: &serde_json::Value, key: &str) -> u32 {
    v.get(key).and_then(|x| x.as_u64()).unwrap_or(0) as u32
}

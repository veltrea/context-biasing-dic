//! Zenn 非公式 API のテキストソース（SPEC 6 章）。
//!
//! 非公式ゆえ、行儀の良さを構造で強制する:
//! - リクエストは最大 1 req/s（毎リクエスト前に 1 秒スリープ）
//! - ツール名を明示した User-Agent
//! - 記事詳細は `articles/zenn/{slug}.json` に保存し、キャッシュ命中時は
//!   リクエストゼロ
//! - 使うのは一覧と詳細の 2 つの JSON エンドポイントのみ（ページの
//!   スクレイピングはしない）
//!
//! 一覧はメタデータのみ（本文なし）で、本文（`body_html`）は記事ごとの
//! 詳細エンドポイントから取る。API の形が変わった場合のフォールバックは
//! RSS（SPEC 6 章）だが、それはその時に実装する。

use crate::source::{Article, Body, TextSource};
use crate::source_qiita::USER_AGENT;
use anyhow::{Context, Result};
use std::fs;
use std::path::PathBuf;
use std::thread;
use std::time::{Duration, Instant};

pub struct ZennSource {
    /// トピック名（例: `rust`）。
    pub topic: String,
    /// 並び順（`daily` | `weekly` | `monthly` | `alltime` など。検証済みは weekly）。
    pub order: String,
    /// キャッシュルート（`articles/zenn/` を下に作る）。
    pub cache_dir: PathBuf,
}

impl TextSource for ZennSource {
    fn fetch(&self, n: usize) -> Result<Vec<Article>> {
        let started = Instant::now();
        let throttle = Duration::from_secs(1);

        // 一覧（メタのみ）。これも 1 リクエストなのでスロットルを通す。
        thread::sleep(throttle);
        eprintln!(
            "zenn: [{:5.1}s] GET /api/articles?order={}&topicname={}",
            started.elapsed().as_secs_f32(),
            self.order,
            self.topic
        );
        let body = ureq::get("https://zenn.dev/api/articles")
            .query("order", &self.order)
            .query("topicname", &self.topic)
            .query("count", &n.clamp(1, 100).to_string())
            .set("User-Agent", USER_AGENT)
            .call()
            .context("Zenn list API request failed (unofficial API changed?)")?
            .into_string()
            .context("failed to read Zenn list response")?;
        let listing: serde_json::Value =
            serde_json::from_str(&body).context("Zenn list API returned non-JSON")?;
        let Some(metas) = listing.get("articles").and_then(|a| a.as_array()) else {
            anyhow::bail!("Zenn list API returned unexpected shape (no `articles` array)");
        };

        let article_dir = self.cache_dir.join("articles").join("zenn");
        fs::create_dir_all(&article_dir)
            .with_context(|| format!("failed to create {}", article_dir.display()))?;

        let mut out = Vec::new();
        for meta in metas.iter().take(n) {
            let slug = str_of(meta, "slug");
            if slug.is_empty() {
                continue;
            }
            let liked = u32_of(meta, "liked_count");

            // 詳細: キャッシュ命中ならリクエストゼロ。
            let path = article_dir.join(format!("{slug}.json"));
            let detail: serde_json::Value = if path.exists() {
                let Ok(text) = fs::read_to_string(&path) else {
                    continue;
                };
                let Ok(v) = serde_json::from_str(&text) else {
                    continue;
                };
                v
            } else {
                thread::sleep(throttle);
                eprintln!(
                    "zenn: [{:5.1}s] GET /api/articles/{}",
                    started.elapsed().as_secs_f32(),
                    slug
                );
                // 1 記事の失敗で収穫全体を止めない（warn して次へ）。
                let text = match ureq::get(&format!("https://zenn.dev/api/articles/{slug}"))
                    .set("User-Agent", USER_AGENT)
                    .call()
                {
                    Ok(r) => match r.into_string() {
                        Ok(t) => t,
                        Err(e) => {
                            eprintln!("warn: zenn detail read failed for {slug}: {e}");
                            continue;
                        }
                    },
                    Err(e) => {
                        eprintln!("warn: zenn detail fetch failed for {slug}: {e}");
                        continue;
                    }
                };
                let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) else {
                    eprintln!("warn: zenn detail for {slug} is not JSON");
                    continue;
                };
                let _ = fs::write(&path, &text);
                v
            };

            let Some(article) = detail.get("article") else {
                continue;
            };
            let html = str_of(article, "body_html");
            if html.is_empty() {
                continue;
            }
            out.push(Article {
                id: slug,
                title: str_of(meta, "title"),
                url: format!("https://zenn.dev{}", str_of(meta, "path")),
                popularity: liked,
                body: Body::Html(html),
            });
        }
        Ok(out)
    }

    fn label(&self) -> &str {
        "zenn"
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

//! ローカルファイルのテキストソース。
//!
//! ネットワークに一切触れない完全オフライン経路（SPEC 6 章）。Step 1 の
//! 最小 harvest はこれで一周する。1 ファイル = 1 記事として返し、文への
//! 分解は下流（Step 1 は行分割、Step 2 以降は extract）の責務。

use crate::source::{Article, Body, TextSource};
use anyhow::{Context, Result};
use std::fs;
use std::path::PathBuf;

pub struct FileSource {
    pub path: PathBuf,
}

impl FileSource {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }
}

impl TextSource for FileSource {
    fn fetch(&self, _n: usize) -> Result<Vec<Article>> {
        let text = fs::read_to_string(&self.path)
            .with_context(|| format!("failed to read {}", self.path.display()))?;
        let name = self
            .path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "input".to_string());
        Ok(vec![Article {
            id: name.clone(),
            title: name,
            url: String::new(),
            popularity: 0,
            body: Body::Plain(text),
        }])
    }

    fn label(&self) -> &str {
        "file"
    }

    /// 明示された入力は毎回フル処理（同じ入力 → 同じ辞書の再現を優先）。
    fn dedup_across_runs(&self) -> bool {
        false
    }
}

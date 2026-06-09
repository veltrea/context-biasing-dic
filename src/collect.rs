//! 採用された危険語の頻度集計と、読み不一致ペアの別ログ。
//!
//! 出力は語のみ。元の文・文脈は一切含めない（仕様書 4.4・6）。これにより
//! 「外に出るのは単語だけ」という安全性を仕上げ層でも保つ。

use crate::pipeline::{Candidate, Outcome};
use serde::Serialize;
use std::collections::BTreeMap;
use std::io::{self, Write};

/// 危険語の頻度集計器と、除外ペアの別ログ。
/// 危険語1件の蓄積：読みと出現回数。
#[derive(Debug, Clone)]
struct DangerEntry {
    reading: String,
    count: usize,
}

#[derive(Debug, Default)]
pub struct Collector {
    /// 正解側表記 → (読み, 出現回数)。BTreeMap で表記順を安定させる。
    danger: BTreeMap<String, DangerEntry>,
    /// 読み不一致ペア（滑舌・ノイズ由来）。傾向分析のために捨てずに溜める。
    reject: Vec<Candidate>,
}

/// Amical バイアシング辞書（JSON）のトップレベル。
///
/// フィールドの宣言順がそのまま JSON のキー順になる
/// （schema → version → field → generator → terms）。`serde_json` は
/// 既定で非 ASCII をエスケープしないので、日本語は生の文字で出る。
#[derive(Serialize)]
struct AmicalDict<'a> {
    /// 固定値。Amical がインポート時の識別・検証に使う。
    schema: &'static str,
    /// スキーマ版。現在は 1。
    version: u32,
    /// 分野ラベル。CLI の `--field` 由来。
    field: &'a str,
    /// 生成元ツール名（版を含む）。
    generator: &'static str,
    /// 危険語（count 降順・同数は word 昇順）。
    terms: Vec<AmicalTerm>,
}

/// `terms` の1要素：語と出現回数のみ（文・読み・文脈は持たない）。
#[derive(Serialize)]
struct AmicalTerm {
    word: String,
    count: usize,
}

impl Collector {
    pub fn new() -> Self {
        Self::default()
    }

    /// 分類結果を1件取り込む。
    pub fn add(&mut self, outcome: Outcome) {
        match outcome {
            Outcome::Homophone(c) => {
                let entry = self.danger.entry(c.reference_surface).or_insert(DangerEntry {
                    reading: c.reference_reading,
                    count: 0,
                });
                entry.count += 1;
            }
            Outcome::NonHomophone(c) => self.reject.push(c),
        }
    }

    /// 分類結果をまとめて取り込む。
    pub fn add_all(&mut self, outcomes: impl IntoIterator<Item = Outcome>) {
        for o in outcomes {
            self.add(o);
        }
    }

    /// 危険語を頻度降順（同数は表記昇順）で返す。
    pub fn danger_words_sorted(&self) -> Vec<(String, usize)> {
        let mut v: Vec<(String, usize)> =
            self.danger.iter().map(|(k, e)| (k.clone(), e.count)).collect();
        v.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        v
    }

    /// 読み付きで頻度降順に返す（表記, 読み, 回数）。GUI のチップ表示用。
    pub fn danger_entries_sorted(&self) -> Vec<(String, String, usize)> {
        let mut v: Vec<(String, String, usize)> = self
            .danger
            .iter()
            .map(|(k, e)| (k.clone(), e.reading.clone(), e.count))
            .collect();
        v.sort_by(|a, b| b.2.cmp(&a.2).then_with(|| a.0.cmp(&b.0)));
        v
    }

    /// 採用された危険語の異なり数。
    pub fn danger_len(&self) -> usize {
        self.danger.len()
    }

    /// 除外（読み不一致）ペア。
    pub fn reject_pairs(&self) -> &[Candidate] {
        &self.reject
    }

    /// 危険語リストを書き出す。1行1語（語のみ。そのまま ASR の用語集に投入できる）。
    pub fn write_dict(&self, w: &mut impl Write) -> io::Result<()> {
        for (word, _count) in self.danger_words_sorted() {
            writeln!(w, "{}", word)?;
        }
        Ok(())
    }

    /// 頻度付きで書き出す（解析・停止条件の見極め用）。`語\t回数`。
    pub fn write_dict_with_counts(&self, w: &mut impl Write) -> io::Result<()> {
        for (word, count) in self.danger_words_sorted() {
            writeln!(w, "{}\t{}", word, count)?;
        }
        Ok(())
    }

    /// Amical 取り込み用のバイアシング辞書（メタ付き JSON）を書き出す。
    ///
    /// スキーマは固定（`amical-biasing-dictionary` / version 1）。`terms` は
    /// `danger_words_sorted()` と同じ並び（count 降順・同数は word 昇順）で、
    /// 先頭の語ほど Amical 側の system context で生き残る。手書き連結ではなく
    /// `serde_json` で直列化するので、エスケープ・カンマ・順序を取り違えない。
    /// pretty（2 スペース）で出し、末尾に改行を1個足す。危険語0件でも
    /// `"terms": []` の妥当な JSON になる。
    pub fn write_amical_json(&self, w: &mut impl Write, field: &str) -> io::Result<()> {
        let terms = self
            .danger_words_sorted()
            .into_iter()
            .map(|(word, count)| AmicalTerm { word, count })
            .collect();
        let doc = AmicalDict {
            schema: "amical-biasing-dictionary",
            version: 1,
            field,
            generator: concat!("biasdiff ", env!("CARGO_PKG_VERSION")),
            terms,
        };
        // 再借用で w を渡し、直列化後も writeln! に使えるようにする。
        serde_json::to_writer_pretty(&mut *w, &doc).map_err(io::Error::from)?;
        // to_writer_pretty は末尾に改行を付けないので、自前で1個足す。
        writeln!(w)?;
        Ok(())
    }

    /// 読み不一致ペアの別ログを書き出す。`正解表記\t認識表記\t正解読み\t認識読み`。
    /// 文・文脈は含めず、語と読みだけ（傾向分析に使える）。
    pub fn write_reject(&self, w: &mut impl Write) -> io::Result<()> {
        for c in &self.reject {
            writeln!(
                w,
                "{}\t{}\t{}\t{}",
                c.reference_surface, c.hypothesis_surface, c.reference_reading, c.hypothesis_reading
            )?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pipeline::Candidate;

    fn homophone(reference: &str) -> Outcome {
        Outcome::Homophone(Candidate {
            reference_surface: reference.to_string(),
            hypothesis_surface: "誤".to_string(),
            reference_reading: "ヨミ".to_string(),
            hypothesis_reading: "ヨミ".to_string(),
        })
    }

    #[test]
    fn counts_and_sorts_by_frequency() {
        let mut c = Collector::new();
        c.add(homophone("機械"));
        c.add(homophone("機械"));
        c.add(homophone("意思"));

        let sorted = c.danger_words_sorted();
        assert_eq!(sorted[0], ("機械".to_string(), 2));
        assert_eq!(sorted[1], ("意思".to_string(), 1));
        assert_eq!(c.danger_len(), 2);
    }

    #[test]
    fn reject_goes_to_separate_log() {
        let mut c = Collector::new();
        c.add(Outcome::NonHomophone(Candidate {
            reference_surface: "猫".to_string(),
            hypothesis_surface: "犬".to_string(),
            reference_reading: "ネコ".to_string(),
            hypothesis_reading: "イヌ".to_string(),
        }));
        assert_eq!(c.danger_len(), 0);
        assert_eq!(c.reject_pairs().len(), 1);
    }

    #[test]
    fn dict_output_is_words_only() {
        let mut c = Collector::new();
        c.add(homophone("機械"));
        let mut buf = Vec::new();
        c.write_dict(&mut buf).unwrap();
        assert_eq!(String::from_utf8(buf).unwrap(), "機械\n");
    }

    #[test]
    fn amical_json_has_meta_and_count_desc_terms() {
        let mut c = Collector::new();
        c.add(homophone("機械"));
        c.add(homophone("機械"));
        c.add(homophone("意思"));

        let mut buf = Vec::new();
        c.write_amical_json(&mut buf, "dev").unwrap();

        // 末尾改行が1個。
        assert!(buf.ends_with(b"\n"));
        // 日本語はエスケープせず生の文字で出る（\uXXXX を含まない）。
        let text = String::from_utf8(buf.clone()).unwrap();
        assert!(text.contains("機械"));
        assert!(!text.contains("\\u"));

        // パースし直してメタと並び順を検証する。
        let v: serde_json::Value = serde_json::from_slice(&buf).unwrap();
        assert_eq!(v["schema"], "amical-biasing-dictionary");
        assert_eq!(v["version"], 1);
        assert_eq!(v["field"], "dev");
        assert!(v["generator"].as_str().unwrap().starts_with("biasdiff"));

        let terms = v["terms"].as_array().unwrap();
        assert_eq!(terms.len(), 2);
        // count 降順：機械(2) が先、意思(1) が後。
        assert_eq!(terms[0]["word"], "機械");
        assert_eq!(terms[0]["count"], 2);
        assert_eq!(terms[1]["word"], "意思");
        assert_eq!(terms[1]["count"], 1);
    }

    #[test]
    fn amical_json_empty_terms_is_valid() {
        let c = Collector::new();
        let mut buf = Vec::new();
        c.write_amical_json(&mut buf, "general").unwrap();

        let v: serde_json::Value = serde_json::from_slice(&buf).unwrap();
        assert_eq!(v["schema"], "amical-biasing-dictionary");
        assert_eq!(v["field"], "general");
        assert_eq!(v["terms"].as_array().unwrap().len(), 0);
    }
}

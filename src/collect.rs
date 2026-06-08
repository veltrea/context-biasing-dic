//! 採用された危険語の頻度集計と、読み不一致ペアの別ログ。
//!
//! 出力は語のみ。元の文・文脈は一切含めない（仕様書 4.4・6）。これにより
//! 「外に出るのは単語だけ」という安全性を仕上げ層でも保つ。

use crate::pipeline::{Candidate, Outcome};
use std::collections::BTreeMap;
use std::io::{self, Write};

/// 危険語の頻度集計器と、除外ペアの別ログ。
#[derive(Debug, Default)]
pub struct Collector {
    /// 正解側表記 → 出現回数。BTreeMap で表記順を安定させる。
    danger: BTreeMap<String, usize>,
    /// 読み不一致ペア（滑舌・ノイズ由来）。傾向分析のために捨てずに溜める。
    reject: Vec<Candidate>,
}

impl Collector {
    pub fn new() -> Self {
        Self::default()
    }

    /// 分類結果を1件取り込む。
    pub fn add(&mut self, outcome: Outcome) {
        match outcome {
            Outcome::Homophone(c) => {
                *self.danger.entry(c.reference_surface).or_insert(0) += 1;
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
            self.danger.iter().map(|(k, &n)| (k.clone(), n)).collect();
        v.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
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
}

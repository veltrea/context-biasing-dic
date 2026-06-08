//! パイプライン全体（形態素解析 → diff → 読みフィルタ → 集計）の統合テスト。
//!
//! 形態素解析器はモックを注入するので Lindera は不要。`--no-default-features` で
//! 辞書をダウンロードせずに高速に回せる。

use biasdiff::collect::Collector;
use biasdiff::pipeline::process;
use biasdiff::reading::NormalizeOptions;
use biasdiff::token::{Token, Tokenizer};
use std::collections::HashMap;

/// 文字列 → トークン列の対応表を持つテスト用解析器。
struct MockTokenizer {
    table: HashMap<&'static str, Vec<Token>>,
}

impl MockTokenizer {
    fn new() -> Self {
        let mut table = HashMap::new();
        let t = |s: &str, r: &str| Token::new(s, r, "名詞");

        // 機械 / 機会（キカイ）— 完全同音
        table.insert(
            "今日は機械を学ぶ",
            vec![
                t("今日", "キョウ"),
                t("は", "ハ"),
                t("機械", "キカイ"),
                t("を", "ヲ"),
                t("学ぶ", "マナブ"),
            ],
        );
        table.insert(
            "今日は機会を学ぶ",
            vec![
                t("今日", "キョウ"),
                t("は", "ハ"),
                t("機会", "キカイ"),
                t("を", "ヲ"),
                t("学ぶ", "マナブ"),
            ],
        );

        // 化学 / 科学（カガク）— 完全同音
        table.insert("化学の本", vec![t("化学", "カガク"), t("の", "ノ"), t("本", "ホン")]);
        table.insert("科学の本", vec![t("科学", "カガク"), t("の", "ノ"), t("本", "ホン")]);

        // 猫 / 犬（読み違い）— 滑舌・ノイズ由来として除外されるべき
        table.insert("猫が鳴く", vec![t("猫", "ネコ"), t("が", "ガ"), t("鳴く", "ナク")]);
        table.insert("犬が鳴く", vec![t("犬", "イヌ"), t("が", "ガ"), t("鳴く", "ナク")]);

        Self { table }
    }
}

impl Tokenizer for MockTokenizer {
    fn tokenize(&self, text: &str) -> anyhow::Result<Vec<Token>> {
        Ok(self.table.get(text).cloned().unwrap_or_default())
    }
}

#[test]
fn homophone_is_collected() {
    let tk = MockTokenizer::new();
    let opts = NormalizeOptions::loose();

    let outs = process(&tk, "今日は機械を学ぶ", "今日は機会を学ぶ", &opts).unwrap();
    let homophones: Vec<_> = outs.iter().filter(|o| o.is_homophone()).collect();
    assert_eq!(homophones.len(), 1);
    assert_eq!(homophones[0].candidate().reference_surface, "機械");
}

#[test]
fn different_reading_is_rejected() {
    let tk = MockTokenizer::new();
    let opts = NormalizeOptions::loose();

    let outs = process(&tk, "猫が鳴く", "犬が鳴く", &opts).unwrap();
    assert!(outs.iter().all(|o| !o.is_homophone()));
}

#[test]
fn end_to_end_collects_and_counts() {
    let tk = MockTokenizer::new();
    let opts = NormalizeOptions::loose();
    let mut collector = Collector::new();

    // 「機械→機会」を2回、「化学→科学」を1回、「猫→犬」を1回（除外）。
    for (reference, hypothesis) in [
        ("今日は機械を学ぶ", "今日は機会を学ぶ"),
        ("今日は機械を学ぶ", "今日は機会を学ぶ"),
        ("化学の本", "科学の本"),
        ("猫が鳴く", "犬が鳴く"),
    ] {
        collector.add_all(process(&tk, reference, hypothesis, &opts).unwrap());
    }

    let sorted = collector.danger_words_sorted();
    assert_eq!(sorted[0], ("機械".to_string(), 2));
    assert_eq!(sorted[1], ("化学".to_string(), 1));
    assert_eq!(collector.danger_len(), 2);
    assert_eq!(collector.reject_pairs().len(), 1);

    // 出力は語のみ・頻度順。
    let mut buf = Vec::new();
    collector.write_dict(&mut buf).unwrap();
    assert_eq!(String::from_utf8(buf).unwrap(), "機械\n化学\n");
}

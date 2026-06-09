//! 置換ペアを「読みが一致する同音衝突」と「読み違いの素の誤認識」に分類し、
//! 正解文・認識文から危険語候補を取り出すパイプライン。
//!
//! 置換ペアには二種が混ざる（仕様書 4.3）。
//!   1. 読みが同じで表記が違う（実装/失踪）  → 本命の危険語。採用。
//!   2. 読みからして違う（滑舌・ノイズ由来）  → 辞書では直せない。除外して別ログへ。
//! 読み（正規化後）の一致だけを採用条件にすることで、辞書の純度を保つ。

use crate::diff::{diff_rows, extract_replacements, DiffRow, ReplacePair};
use crate::reading::{normalize, NormalizeOptions};
use crate::token::{Token, Tokenizer};

/// 置換ペア1つを語の対として表したもの。読みは正規化後の値を持つ。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    /// 正解側の表記（辞書に採用するのはこちら）。
    pub reference_surface: String,
    /// 認識側の表記（誤変換された側。傾向分析に使う）。
    pub hypothesis_surface: String,
    /// 正解側の読み（正規化後）。
    pub reference_reading: String,
    /// 認識側の読み（正規化後）。
    pub hypothesis_reading: String,
}

/// 置換ペアの分類結果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// 読みが一致＝同音衝突。危険語候補として採用する。
    Homophone(Candidate),
    /// 読みが違う＝滑舌・ノイズ由来。辞書では直せないので除外（別ログ行き）。
    NonHomophone(Candidate),
}

impl Outcome {
    pub fn candidate(&self) -> &Candidate {
        match self {
            Outcome::Homophone(c) | Outcome::NonHomophone(c) => c,
        }
    }
    pub fn is_homophone(&self) -> bool {
        matches!(self, Outcome::Homophone(_))
    }
}

fn join_surface(tokens: &[Token]) -> String {
    tokens.iter().map(|t| t.surface.as_str()).collect()
}

fn join_reading(tokens: &[Token], opts: &NormalizeOptions) -> String {
    tokens.iter().map(|t| normalize(&t.reading, opts)).collect()
}

/// 置換ペアを読み一致で分類する。
///
/// 正解側・認識側それぞれのトークン読みを正規化して連結し、突き合わせる。
/// 連結することで「1語 vs 複数語」のような区切り違いの置換も自然に扱える。
pub fn classify(pair: &ReplacePair, opts: &NormalizeOptions) -> Outcome {
    let cand = Candidate {
        reference_surface: join_surface(&pair.reference),
        hypothesis_surface: join_surface(&pair.hypothesis),
        reference_reading: join_reading(&pair.reference, opts),
        hypothesis_reading: join_reading(&pair.hypothesis, opts),
    };

    // 読みが空（記号など読みを持たない置換）は同音とみなさない。
    let same_reading = !cand.reference_reading.is_empty()
        && cand.reference_reading == cand.hypothesis_reading;

    if same_reading {
        Outcome::Homophone(cand)
    } else {
        Outcome::NonHomophone(cand)
    }
}

/// 正解文と認識文を形態素解析し、置換ペアを分類して返す。
///
/// 形態素解析器は `Tokenizer` トレイト越しに受け取る（依存性注入）ため、
/// テストではモックを渡せる。
pub fn process(
    tokenizer: &dyn Tokenizer,
    reference_text: &str,
    hypothesis_text: &str,
    opts: &NormalizeOptions,
) -> anyhow::Result<Vec<Outcome>> {
    let ref_tokens = tokenizer.tokenize(reference_text)?;
    let hyp_tokens = tokenizer.tokenize(hypothesis_text)?;
    let pairs = extract_replacements(&ref_tokens, &hyp_tokens);
    Ok(pairs.iter().map(|p| classify(p, opts)).collect())
}

/// 行ごとの分類。Equal はそのまま、置換は同音/非同音に分ける（GUI 表示用）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RowOutcome {
    /// 一致行（連結表記）。
    Equal { surface: String },
    /// 採用：同音衝突。
    Homophone(Candidate),
    /// 除外：読み違い。
    NonHomophone(Candidate),
}

fn classify_row(row: DiffRow, opts: &NormalizeOptions) -> RowOutcome {
    match row {
        DiffRow::Equal(tokens) => RowOutcome::Equal {
            surface: tokens.iter().map(|t| t.surface.as_str()).collect(),
        },
        DiffRow::Replace(pair) => match classify(&pair, opts) {
            Outcome::Homophone(c) => RowOutcome::Homophone(c),
            Outcome::NonHomophone(c) => RowOutcome::NonHomophone(c),
        },
    }
}

/// 正解文と認識文を形態素解析し、一致行も含めた全行を分類して返す。
/// `process` が置換ペアだけを返すのに対し、こちらは Equal 行も含む（GUI 用）。
pub fn process_rows(
    tokenizer: &dyn Tokenizer,
    reference_text: &str,
    hypothesis_text: &str,
    opts: &NormalizeOptions,
) -> anyhow::Result<Vec<RowOutcome>> {
    let ref_tokens = tokenizer.tokenize(reference_text)?;
    let hyp_tokens = tokenizer.tokenize(hypothesis_text)?;
    let rows = diff_rows(&ref_tokens, &hyp_tokens);
    Ok(rows.into_iter().map(|row| classify_row(row, opts)).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diff::ReplacePair;

    fn tok(surface: &str, reading: &str) -> Token {
        Token::new(surface, reading, "名詞")
    }

    #[test]
    fn same_reading_is_homophone() {
        // 機械 / 機会 はどちらも キカイ → 同音衝突
        let pair = ReplacePair {
            reference: vec![tok("機械", "キカイ")],
            hypothesis: vec![tok("機会", "キカイ")],
        };
        let out = classify(&pair, &NormalizeOptions::loose());
        assert!(out.is_homophone());
        assert_eq!(out.candidate().reference_surface, "機械");
    }

    #[test]
    fn different_reading_is_rejected() {
        // 読みからして違う → 除外
        let pair = ReplacePair {
            reference: vec![tok("猫", "ネコ")],
            hypothesis: vec![tok("犬", "イヌ")],
        };
        let out = classify(&pair, &NormalizeOptions::loose());
        assert!(!out.is_homophone());
    }

    #[test]
    fn dakuten_yure_is_homophone_when_loose() {
        // 濁点ゆれ。loose では同音扱い、strict では別音扱い。
        let pair = ReplacePair {
            reference: vec![tok("実装", "ジッソウ")],
            hypothesis: vec![tok("失踪", "シッソウ")],
        };
        assert!(classify(&pair, &NormalizeOptions::loose()).is_homophone());
        assert!(!classify(&pair, &NormalizeOptions::strict()).is_homophone());
    }

    #[test]
    fn multi_token_reading_is_concatenated() {
        // 「形態素」 vs 「形態」「素」 は連結すれば同読み
        let pair = ReplacePair {
            reference: vec![tok("形態素", "ケイタイソ")],
            hypothesis: vec![tok("形態", "ケイタイ"), tok("素", "ソ")],
        };
        let out = classify(&pair, &NormalizeOptions::loose());
        assert!(out.is_homophone());
        assert_eq!(out.candidate().reference_surface, "形態素");
    }
}

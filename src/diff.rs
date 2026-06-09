//! トークン列の diff から置換（replace）ブロックを抽出する。
//!
//! 文字単位だと「語」として扱いにくいため、表記（surface）の列同士で Myers diff を取る。
//! 関心があるのは置換ブロックだけ。挿入・削除は誤変換というより区切りのズレが多いので
//! 採らない（仕様書 4.2）。

use crate::token::Token;
use similar::{capture_diff_slices, Algorithm, DiffOp};

/// 置換1ブロック。正解側と認識側のトークン列（1対1とは限らない）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReplacePair {
    /// 正解側トークン列。
    pub reference: Vec<Token>,
    /// 認識側トークン列。
    pub hypothesis: Vec<Token>,
}

/// 正解トークン列と認識トークン列を diff し、置換ブロックだけを取り出す。
///
/// 比較キーは表記（surface）。読みではなく表記で diff することで、
/// 「表記が違う＝何かしら食い違った箇所」を置換として拾える。
/// その置換が同音なのか別音なのかは後段（`pipeline`）で読みを見て判定する。
pub fn extract_replacements(reference: &[Token], hypothesis: &[Token]) -> Vec<ReplacePair> {
    let ref_surfaces: Vec<&str> = reference.iter().map(|t| t.surface.as_str()).collect();
    let hyp_surfaces: Vec<&str> = hypothesis.iter().map(|t| t.surface.as_str()).collect();

    let ops = capture_diff_slices(Algorithm::Myers, &ref_surfaces, &hyp_surfaces);

    let mut pairs = Vec::new();
    for op in ops {
        if let DiffOp::Replace {
            old_index,
            old_len,
            new_index,
            new_len,
        } = op
        {
            pairs.push(ReplacePair {
                reference: reference[old_index..old_index + old_len].to_vec(),
                hypothesis: hypothesis[new_index..new_index + new_len].to_vec(),
            });
        }
    }
    pairs
}

/// diff の1行。Equal（一致）か Replace（置換）。挿入・削除は採らない。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiffRow {
    /// 一致したトークン列。
    Equal(Vec<Token>),
    /// 置換ブロック。
    Replace(ReplacePair),
}

/// 表記列で diff を取り、Equal と Replace を順に返す（挿入・削除はスキップ）。
///
/// `extract_replacements` が置換だけを返すのに対し、こちらは一致行も含めて
/// 差分の全体像を返す（GUI で「＝」の一致行も見せるため）。
pub fn diff_rows(reference: &[Token], hypothesis: &[Token]) -> Vec<DiffRow> {
    let ref_surfaces: Vec<&str> = reference.iter().map(|t| t.surface.as_str()).collect();
    let hyp_surfaces: Vec<&str> = hypothesis.iter().map(|t| t.surface.as_str()).collect();

    let ops = capture_diff_slices(Algorithm::Myers, &ref_surfaces, &hyp_surfaces);

    let mut rows = Vec::new();
    for op in ops {
        match op {
            DiffOp::Equal { old_index, len, .. } => {
                rows.push(DiffRow::Equal(reference[old_index..old_index + len].to_vec()));
            }
            DiffOp::Replace {
                old_index,
                old_len,
                new_index,
                new_len,
            } => {
                rows.push(DiffRow::Replace(ReplacePair {
                    reference: reference[old_index..old_index + old_len].to_vec(),
                    hypothesis: hypothesis[new_index..new_index + new_len].to_vec(),
                }));
            }
            DiffOp::Insert { .. } | DiffOp::Delete { .. } => {}
        }
    }
    rows
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tok(surface: &str, reading: &str) -> Token {
        Token::new(surface, reading, "名詞")
    }

    #[test]
    fn extracts_single_replacement() {
        // 「機械 を 学ぶ」 vs 「機会 を 学ぶ」 → 置換は 機械→機会 の1つ
        let reference = vec![tok("機械", "キカイ"), tok("を", "ヲ"), tok("学ぶ", "マナブ")];
        let hypothesis = vec![tok("機会", "キカイ"), tok("を", "ヲ"), tok("学ぶ", "マナブ")];

        let pairs = extract_replacements(&reference, &hypothesis);
        assert_eq!(pairs.len(), 1);
        assert_eq!(pairs[0].reference, vec![tok("機械", "キカイ")]);
        assert_eq!(pairs[0].hypothesis, vec![tok("機会", "キカイ")]);
    }

    #[test]
    fn no_diff_yields_no_pairs() {
        let reference = vec![tok("東京", "トウキョウ"), tok("駅", "エキ")];
        let hypothesis = vec![tok("東京", "トウキョウ"), tok("駅", "エキ")];
        assert!(extract_replacements(&reference, &hypothesis).is_empty());
    }

    #[test]
    fn captures_multi_token_replacement() {
        // 区切りの違う置換も1ブロックとしてまとめて取れる
        let reference = vec![tok("形態素", "ケイタイソ"), tok("解析", "カイセキ")];
        let hypothesis = vec![
            tok("形態", "ケイタイ"),
            tok("祖", "ソ"),
            tok("回析", "カイセキ"),
        ];
        let pairs = extract_replacements(&reference, &hypothesis);
        assert_eq!(pairs.len(), 1);
        assert_eq!(pairs[0].reference.len(), 2);
        assert_eq!(pairs[0].hypothesis.len(), 3);
    }
}

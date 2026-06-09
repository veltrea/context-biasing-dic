//! 多声の頑健性投票（SPEC 11 章）。
//!
//! 声マトリクスの各セルで観測された置換ペアを溜め、同音ペアは「少なくとも
//! `min_votes` 個の**異なる話者**で観測された」ものだけを `Collector` へ流す。
//! エンジン固有の TTS の癖（特定話者だけが起こす誤読由来のペア）が単独で
//! 辞書エントリを作ることを防ぐ。読み不一致（NonHomophone）は投票対象外で
//! 素通しする — 除外ログは傾向分析用で、削る理由がないため。
//!
//! 投票は `Collector::add` の前段に置き、通過後の頻度集計の意味は変えない:
//! あるペアが 5 回観測され投票を通れば、Collector には 5 回 add される。

use crate::collect::Collector;
use crate::pipeline::Outcome;
use std::collections::{BTreeMap, BTreeSet};

/// 投票の集計簿。`record` で観測を溜め、`flush_into` で閾値を適用して流し込む。
#[derive(Debug, Default)]
pub struct VoteBook {
    /// (正解表記, 認識表記) → 観測。同音ペアのみ（投票の対象）。
    entries: BTreeMap<(String, String), Entry>,
    /// 読み不一致。投票せずそのまま Collector へ渡す。
    rejects: Vec<Outcome>,
}

#[derive(Debug, Default)]
struct Entry {
    /// このペアを観測した話者（`VoiceSpec::speaker_key`）の集合。
    speakers: BTreeSet<String>,
    /// 観測された Outcome 列。通過時はすべて Collector へ流す（頻度を保つ）。
    outcomes: Vec<Outcome>,
}

/// `flush_into` の集計結果。投票が何を削ったかを呼び出し側が報告できるようにする。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VoteSummary {
    /// 採用された同音ペアの異なり数。
    pub passed_pairs: usize,
    /// 票が足りず落ちた同音ペアの異なり数。
    pub dropped_pairs: usize,
    /// 落ちたペアの内訳（正解表記, 認識表記, 観測話者数）。ログ表示用。
    pub dropped: Vec<(String, String, usize)>,
}

impl VoteBook {
    pub fn new() -> Self {
        Self::default()
    }

    /// 1 セル分の分類結果を記録する。`speaker` は `VoiceSpec::speaker_key()`。
    pub fn record(&mut self, speaker: &str, outcomes: impl IntoIterator<Item = Outcome>) {
        for o in outcomes {
            match &o {
                Outcome::Homophone(c) => {
                    let key = (c.reference_surface.clone(), c.hypothesis_surface.clone());
                    let e = self.entries.entry(key).or_default();
                    e.speakers.insert(speaker.to_string());
                    e.outcomes.push(o);
                }
                Outcome::NonHomophone(_) => self.rejects.push(o),
            }
        }
    }

    /// 閾値を適用して Collector へ流し込む。
    pub fn flush_into(self, collector: &mut Collector, min_votes: usize) -> VoteSummary {
        let mut summary = VoteSummary {
            passed_pairs: 0,
            dropped_pairs: 0,
            dropped: Vec::new(),
        };
        for ((reference, hypothesis), entry) in self.entries {
            if entry.speakers.len() >= min_votes {
                summary.passed_pairs += 1;
                collector.add_all(entry.outcomes);
            } else {
                summary.dropped_pairs += 1;
                summary
                    .dropped
                    .push((reference, hypothesis, entry.speakers.len()));
            }
        }
        collector.add_all(self.rejects);
        summary
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pipeline::Candidate;

    fn homophone(reference: &str, hypothesis: &str) -> Outcome {
        Outcome::Homophone(Candidate {
            reference_surface: reference.to_string(),
            hypothesis_surface: hypothesis.to_string(),
            reference_reading: "ヨミ".to_string(),
            hypothesis_reading: "ヨミ".to_string(),
        })
    }

    fn rejected(reference: &str, hypothesis: &str) -> Outcome {
        Outcome::NonHomophone(Candidate {
            reference_surface: reference.to_string(),
            hypothesis_surface: hypothesis.to_string(),
            reference_reading: "ア".to_string(),
            hypothesis_reading: "イ".to_string(),
        })
    }

    #[test]
    fn single_speaker_pair_drops_at_min_votes_2() {
        let mut book = VoteBook::new();
        // 話者 A だけが 3 回観測（同じ話者は何回観測しても 1 票）。
        book.record("voicevox:3", vec![homophone("機械", "機会")]);
        book.record("voicevox:3", vec![homophone("機械", "機会")]);
        book.record("voicevox:3", vec![homophone("機械", "機会")]);

        let mut c = Collector::new();
        let s = book.flush_into(&mut c, 2);
        assert_eq!(s.passed_pairs, 0);
        assert_eq!(s.dropped_pairs, 1);
        assert_eq!(s.dropped, vec![("機械".to_string(), "機会".to_string(), 1)]);
        assert_eq!(c.danger_len(), 0);
    }

    #[test]
    fn two_speakers_pass_and_frequency_is_kept() {
        let mut book = VoteBook::new();
        // 話者 A で 2 回・話者 B で 1 回 → 2 票で採用、頻度は 3。
        book.record("voicevox:3", vec![homophone("機械", "機会")]);
        book.record("voicevox:3", vec![homophone("機械", "機会")]);
        book.record("say:Kyoko", vec![homophone("機械", "機会")]);

        let mut c = Collector::new();
        let s = book.flush_into(&mut c, 2);
        assert_eq!(s.passed_pairs, 1);
        assert_eq!(s.dropped_pairs, 0);
        assert_eq!(c.danger_words_sorted(), vec![("機械".to_string(), 3)]);
    }

    #[test]
    fn min_votes_1_passes_everything() {
        let mut book = VoteBook::new();
        book.record("voicevox:3", vec![homophone("意思", "医師")]);

        let mut c = Collector::new();
        let s = book.flush_into(&mut c, 1);
        assert_eq!(s.passed_pairs, 1);
        assert_eq!(c.danger_len(), 1);
    }

    #[test]
    fn rejects_bypass_voting() {
        let mut book = VoteBook::new();
        // 読み不一致は 1 話者でも素通し（除外ログは傾向分析用）。
        book.record("voicevox:3", vec![rejected("機械", "非会")]);

        let mut c = Collector::new();
        book.flush_into(&mut c, 2);
        assert_eq!(c.reject_pairs().len(), 1);
        assert_eq!(c.danger_len(), 0);
    }

    #[test]
    fn same_surface_pair_from_different_hypotheses_vote_separately() {
        let mut book = VoteBook::new();
        // 「対照→対象」と「対称→対象」は別ペアとして別々に投票される。
        book.record("voicevox:3", vec![homophone("対照", "対象")]);
        book.record("say:Kyoko", vec![homophone("対称", "対象")]);

        let mut c = Collector::new();
        let s = book.flush_into(&mut c, 2);
        assert_eq!(s.passed_pairs, 0);
        assert_eq!(s.dropped_pairs, 2);
    }
}

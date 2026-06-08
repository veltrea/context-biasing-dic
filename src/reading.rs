//! 読み（カナ）の正規化。同音判定の純度を決める一段。
//!
//! 同じ辞書から引けば同音異義語の読みは基本一致するが、長音・促音・濁点の
//! 表記ゆれで取りこぼすことがある。仕様書 4.3 の「正規化したうえでの一致」を
//! 担うのがこのモジュール。`strict()` は完全一致寄り、`loose()` はゆれを畳み込む。

/// 正規化オプション。各ゆれを畳み込むかどうか。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NormalizeOptions {
    /// 濁点・半濁点を除去して畳み込む（ガ→カ, パ→ハ）。
    pub fold_dakuten: bool,
    /// 長音のゆれを畳み込む（母音長音「ケイ」と長音記号「ケー」を統一）。
    pub fold_prolonged: bool,
    /// 促音「ッ」を畳み込む（除去）。
    pub fold_sokuon: bool,
    /// 小書き仮名を通常サイズへ畳み込む（ァ→ア, ャ→ヤ）。
    pub fold_small: bool,
}

impl NormalizeOptions {
    /// 完全一致寄り。カタカナ統一のみ行い、ゆれは畳み込まない。
    pub fn strict() -> Self {
        Self {
            fold_dakuten: false,
            fold_prolonged: false,
            fold_sokuon: false,
            fold_small: false,
        }
    }

    /// ゆれ吸収あり。長音・促音・濁点・小書きを畳み込む（取りこぼしを減らす）。
    pub fn loose() -> Self {
        Self {
            fold_dakuten: true,
            fold_prolonged: true,
            fold_sokuon: true,
            fold_small: true,
        }
    }
}

impl Default for NormalizeOptions {
    fn default() -> Self {
        Self::loose()
    }
}

/// カナの母音（段）。長音畳み込みの判定に使う。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Vowel {
    A,
    I,
    U,
    E,
    O,
}

/// ひらがなをカタカナへ変換する。それ以外の文字はそのまま。
pub fn to_katakana(s: &str) -> String {
    s.chars()
        .map(|c| {
            let u = c as u32;
            // ひらがな U+3041..=U+3096 を +0x60 してカタカナへ。
            if (0x3041..=0x3096).contains(&u) {
                char::from_u32(u + 0x60).unwrap_or(c)
            } else {
                c
            }
        })
        .collect()
}

/// 読みを正規化する。まずカタカナへ統一し、オプションに応じてゆれを畳み込む。
///
/// 長音の畳み込みは「直前の段＋後続母音」が長音化ペアなら長音記号「ー」へ統一する。
/// 例: 「ケイタイ」→「ケータイ」、「ケータイ」→「ケータイ」となり一致する。
pub fn normalize(reading: &str, opts: &NormalizeOptions) -> String {
    let kata = to_katakana(reading);
    let mut out = String::with_capacity(kata.len());
    // 長音判定のための直前の段（母音）。ンや記号でリセットされる。
    let mut prev_vowel: Option<Vowel> = None;

    for ch in kata.chars() {
        let mut c = ch;
        if opts.fold_dakuten {
            c = strip_dakuten(c);
        }
        if opts.fold_small {
            c = enlarge_small(c);
        }
        if opts.fold_sokuon && c == 'ッ' {
            // 促音は落とす。段は更新しない（前後の母音連結に影響させない）。
            continue;
        }

        if opts.fold_prolonged {
            if c == 'ー' {
                // 長音記号。直前に段があるときだけ「ー」を1つ出す。段は維持。
                if prev_vowel.is_some() {
                    out.push('ー');
                }
                continue;
            }
            // 長音化するのは「母音単体（ア/イ/ウ/エ/オ）」が直前モーラと母音連続を
            // 成すときだけ。子音モーラ（カ・キ…）は同じ段が続いても長音にしない
            // （カガク→カカクであって カーク ではない）。
            if is_standalone_vowel(c) {
                let cv = vowel_of(c).expect("standalone vowel always has a vowel");
                if let Some(pv) = prev_vowel {
                    if is_long_pair(pv, cv) {
                        out.push('ー'); // 段は据え置く
                        continue;
                    }
                }
                out.push(c);
                prev_vowel = Some(cv);
                continue;
            }
            // 子音モーラ等。そのまま出し、段を更新（撥音・記号なら None でリセット）。
            out.push(c);
            prev_vowel = vowel_of(c);
            continue;
        }

        out.push(c);
    }

    out
}

/// 濁音・半濁音を清音へ畳み込む。
fn strip_dakuten(c: char) -> char {
    match c {
        'ガ' => 'カ',
        'ギ' => 'キ',
        'グ' => 'ク',
        'ゲ' => 'ケ',
        'ゴ' => 'コ',
        'ザ' => 'サ',
        'ジ' => 'シ',
        'ズ' => 'ス',
        'ゼ' => 'セ',
        'ゾ' => 'ソ',
        'ダ' => 'タ',
        'ヂ' => 'チ',
        'ヅ' => 'ツ',
        'デ' => 'テ',
        'ド' => 'ト',
        'バ' | 'パ' => 'ハ',
        'ビ' | 'ピ' => 'ヒ',
        'ブ' | 'プ' => 'フ',
        'ベ' | 'ペ' => 'ヘ',
        'ボ' | 'ポ' => 'ホ',
        'ヴ' => 'ウ',
        other => other,
    }
}

/// 小書き仮名を通常サイズへ畳み込む（促音「ッ」は別扱いのため触らない）。
fn enlarge_small(c: char) -> char {
    match c {
        'ァ' => 'ア',
        'ィ' => 'イ',
        'ゥ' => 'ウ',
        'ェ' => 'エ',
        'ォ' => 'オ',
        'ャ' => 'ヤ',
        'ュ' => 'ユ',
        'ョ' => 'ヨ',
        'ヮ' => 'ワ',
        'ヵ' => 'カ',
        'ヶ' => 'ケ',
        other => other,
    }
}

/// カナの段（母音）を返す。促音・撥音・長音・記号は None。
fn vowel_of(c: char) -> Option<Vowel> {
    match c {
        'ア' | 'カ' | 'サ' | 'タ' | 'ナ' | 'ハ' | 'マ' | 'ヤ' | 'ラ' | 'ワ' | 'ガ' | 'ザ'
        | 'ダ' | 'バ' | 'パ' | 'ァ' | 'ャ' | 'ヮ' | 'ヵ' => Some(Vowel::A),
        'イ' | 'キ' | 'シ' | 'チ' | 'ニ' | 'ヒ' | 'ミ' | 'リ' | 'ヰ' | 'ギ' | 'ジ' | 'ヂ'
        | 'ビ' | 'ピ' | 'ィ' => Some(Vowel::I),
        'ウ' | 'ク' | 'ス' | 'ツ' | 'ヌ' | 'フ' | 'ム' | 'ユ' | 'ル' | 'グ' | 'ズ' | 'ヅ'
        | 'ブ' | 'プ' | 'ヴ' | 'ゥ' | 'ュ' => Some(Vowel::U),
        'エ' | 'ケ' | 'セ' | 'テ' | 'ネ' | 'ヘ' | 'メ' | 'レ' | 'ヱ' | 'ゲ' | 'ゼ' | 'デ'
        | 'ベ' | 'ペ' | 'ェ' | 'ヶ' => Some(Vowel::E),
        'オ' | 'コ' | 'ソ' | 'ト' | 'ノ' | 'ホ' | 'モ' | 'ヨ' | 'ロ' | 'ヲ' | 'ゴ' | 'ゾ'
        | 'ド' | 'ボ' | 'ポ' | 'ォ' | 'ョ' => Some(Vowel::O),
        _ => None,
    }
}

/// 母音単体（ア/イ/ウ/エ/オ）か。長音畳み込みはこの文字に対してのみ働く。
fn is_standalone_vowel(c: char) -> bool {
    matches!(c, 'ア' | 'イ' | 'ウ' | 'エ' | 'オ')
}

/// 直前の段と後続母音が長音化するペアか。
/// 同母音連続（アア等）に加え、エ段＋イ（ケイ→ケー）、オ段＋ウ（コウ→コー）。
fn is_long_pair(prev: Vowel, cur: Vowel) -> bool {
    matches!(
        (prev, cur),
        (Vowel::A, Vowel::A)
            | (Vowel::I, Vowel::I)
            | (Vowel::U, Vowel::U)
            | (Vowel::E, Vowel::E)
            | (Vowel::O, Vowel::O)
            | (Vowel::E, Vowel::I)
            | (Vowel::O, Vowel::U)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hiragana_to_katakana() {
        assert_eq!(to_katakana("けいたい"), "ケイタイ");
        assert_eq!(to_katakana("ケイタイ"), "ケイタイ");
        assert_eq!(to_katakana("形態"), "形態"); // 漢字はそのまま
    }

    #[test]
    fn strict_only_unifies_katakana() {
        let o = NormalizeOptions::strict();
        assert_eq!(normalize("けいたい", &o), "ケイタイ");
        // strict では長音ゆれは畳み込まれない
        assert_ne!(normalize("ケイタイ", &o), normalize("ケータイ", &o));
    }

    #[test]
    fn loose_folds_prolonged_sound() {
        let o = NormalizeOptions::loose();
        // 母音長音と長音記号が一致する
        assert_eq!(normalize("ケイタイ", &o), normalize("ケータイ", &o));
        assert_eq!(normalize("コウコウ", &o), normalize("コーコー", &o));
        assert_eq!(normalize("トオリ", &o), normalize("トーリ", &o));
    }

    #[test]
    fn loose_folds_dakuten() {
        let o = NormalizeOptions::loose();
        // 濁点ゆれを吸収（ジッソウ ≈ シッソウ）
        assert_eq!(normalize("ジッソウ", &o), normalize("シッソウ", &o));
    }

    #[test]
    fn loose_folds_sokuon_and_small() {
        let o = NormalizeOptions::loose();
        assert_eq!(normalize("ガッコウ", &o), normalize("ガコウ", &o));
        assert_eq!(normalize("キャク", &o), normalize("キヤク", &o));
    }

    #[test]
    fn distinct_readings_stay_distinct() {
        let o = NormalizeOptions::loose();
        // 別の音は畳み込んでも一致しない
        assert_ne!(normalize("キカイ", &o), normalize("キカク", &o));
        assert_ne!(normalize("ハシ", &o), normalize("ハナ", &o));
    }

    #[test]
    fn loose_does_not_overfold_same_column_consonants() {
        let o = NormalizeOptions::loose();
        // 同じ段の子音モーラ連続は長音化しない（カガク→カカク。回帰防止）
        assert_eq!(normalize("カガク", &o), "カカク");
        assert_eq!(normalize("イシ", &o), "イシ");
        // 化学(カガク) と 科学(カガク) は同音として一致する
        assert_eq!(normalize("ガッコウ", &o), "カコー");
    }
}

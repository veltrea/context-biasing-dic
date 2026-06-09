//! 例文化: 記事本文 → TTS に読ませられる文（SPEC 7 章）。
//!
//! 技術記事はコードフェンス・URL・英語識別子だらけで、素通しすると後段の
//! どのフィルタでも除去できないゴミ置換ペアを生む。**辞書の純度はここで
//! 決まる**。段階は: 構造除去 → 文分割 → フィルタ → スコアリング →
//! 重複排除。指針は「迷ったら捨てる」— 記事は豊富にあり、重要なのは精度。
//!
//! このモジュールは純粋（std のみ・正規表現も不使用）。ネットワーク・辞書・
//! 音声なしに網羅的へ単体テストできる（決定 D5）。

use crate::source::Body;
use std::cmp::Ordering;
use std::collections::BTreeSet;

/// 例文化の閾値。CLI オプションにはせずコード内定数で調整する
/// （過剰なオプション化をしない — 実装計画書 § 4 のリスク欄）。
pub struct ExtractOptions {
    /// 文長の下限（TTS 1 発話・diff 1 行として意味を持つ長さ）。
    pub min_len: usize,
    /// 文長の上限。
    pub max_len: usize,
    /// 日本語文字率の下限。
    pub min_japanese_ratio: f64,
    /// 残骸文字・括弧バランスの検査を行うか。
    pub check_residue: bool,
    /// 1 記事から採る文数の上限（長い記事が収穫を支配しないように）。
    pub max_per_article: usize,
}

impl Default for ExtractOptions {
    fn default() -> Self {
        Self {
            min_len: 20,
            max_len: 80,
            min_japanese_ratio: 0.5,
            check_residue: true,
            max_per_article: 20,
        }
    }
}

impl ExtractOptions {
    /// 明示入力（FileSource など `trusted_input` なソース）用の緩いプロファイル。
    /// 構造除去・文分割・重複排除だけ行い、品質フィルタ（文長・日本語率・
    /// 残骸・記事内上限）はかけない — ユーザーが読ませると決めた文を
    /// 黙って捨てない。Plain 本文ならば実質「行分割 + trim + 重複排除」になる。
    pub fn lenient() -> Self {
        Self {
            min_len: 1,
            max_len: usize::MAX,
            min_japanese_ratio: 0.0,
            check_residue: false,
            max_per_article: usize::MAX,
        }
    }
}

/// 記事本文 1 本から文を抽出する。記事内の重複は正規化キーで排除済み。
/// `max_per_article` を超える場合は漢字密度スコアの高い文を優先して残す
/// （同音衝突は漢語に集中する — SPEC 7 章）。出力順は常に記事内の出現順。
pub fn extract(body: &Body, opts: &ExtractOptions) -> Vec<String> {
    let plain = match body {
        Body::Markdown(s) => strip_markdown(s),
        Body::Html(s) => strip_html(s),
        Body::Plain(s) => s.clone(),
    };

    let mut seen: BTreeSet<String> = BTreeSet::new();
    // (漢字率, 出現順, 文)。上限を超えたときだけスコアで選抜する。
    let mut scored: Vec<(f64, usize, String)> = Vec::new();
    for (idx, sent) in split_sentences(&plain).into_iter().enumerate() {
        if !passes_filters(&sent, opts) {
            continue;
        }
        if !seen.insert(normalize_sentence(&sent)) {
            continue;
        }
        scored.push((kanji_ratio(&sent), idx, sent));
    }

    if scored.len() > opts.max_per_article {
        scored.sort_by(|a, b| {
            b.0.partial_cmp(&a.0)
                .unwrap_or(Ordering::Equal)
                .then(a.1.cmp(&b.1))
        });
        scored.truncate(opts.max_per_article);
        // 選抜後は出現順に戻す（dry-run の読みやすさ・処理順の自然さ）。
        scored.sort_by_key(|x| x.1);
    }
    scored.into_iter().map(|(_, _, s)| s).collect()
}

/// 重複排除用の正規化キー: 空白（半角・全角）を除いた文字列。
/// 実行間の永続化（seen.jsonl）ではこのキーをさらにハッシュ化して使う
/// （ハッシュ関数は安定性が要るのでキャッシュ層 = harvest の責務）。
pub fn normalize_sentence(s: &str) -> String {
    s.chars().filter(|c| !c.is_whitespace()).collect()
}

// ---- 構造除去（Markdown） ----

/// Markdown を平文へ。行単位の構造（フェンス・表・見出し・引用・リスト）を
/// 落とし、行内構造（リンク・画像・URL・コード記号・強調）を剥がす。
fn strip_markdown(s: &str) -> String {
    let mut out = String::new();
    let mut in_fence = false;
    for line in s.lines() {
        let trimmed = line.trim_start();

        // コードフェンス。フェンス行自体も中身も捨てる。
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            in_fence = !in_fence;
            continue;
        }
        if in_fence {
            continue;
        }
        // 表の行（セル区切り・罫線）。
        if trimmed.starts_with('|') {
            continue;
        }

        // 見出し記号・引用記号・リスト記号を剥がす（中身の文は残す）。
        let mut rest = trimmed;
        while let Some(r) = rest.strip_prefix('#') {
            rest = r;
        }
        let mut rest = rest.trim_start();
        while let Some(r) = rest.strip_prefix('>') {
            rest = r.trim_start();
        }
        rest = strip_list_marker(rest);

        out.push_str(&strip_inline(rest));
        out.push('\n');
    }
    out
}

/// リスト記号（`- ` / `* ` / `+ ` / `1. `）を 1 段だけ剥がす。
fn strip_list_marker(s: &str) -> &str {
    for m in ["- ", "* ", "+ "] {
        if let Some(r) = s.strip_prefix(m) {
            return r;
        }
    }
    let digits = s.chars().take_while(|c| c.is_ascii_digit()).count();
    if digits > 0 {
        if let Some(r) = s[digits..].strip_prefix(". ") {
            return r;
        }
    }
    s
}

/// 行内構造を剥がす: 画像→消す、リンク→表示文字列、URL→消す、
/// バッククォート・アスタリスク→消す（中身は残し、ゴミはフィルタに任せる）。
fn strip_inline(s: &str) -> String {
    let s = strip_links(s);
    let s = strip_urls(&s);
    s.chars().filter(|c| !matches!(c, '`' | '*')).collect()
}

/// `[text](url)` → `text`、`![alt](url)` → 空。ネストは扱わない（実用十分）。
fn strip_links(s: &str) -> String {
    let chars: Vec<char> = s.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < chars.len() {
        let is_image = chars[i] == '!' && chars.get(i + 1) == Some(&'[');
        let bracket = if is_image {
            Some(i + 1)
        } else if chars[i] == '[' {
            Some(i)
        } else {
            None
        };
        if let Some(b) = bracket {
            if let Some(close) = find_char(&chars, b + 1, ']') {
                if chars.get(close + 1) == Some(&'(') {
                    if let Some(paren) = find_char(&chars, close + 2, ')') {
                        if !is_image {
                            out.extend(&chars[b + 1..close]);
                        }
                        i = paren + 1;
                        continue;
                    }
                }
            }
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}

fn find_char(chars: &[char], from: usize, target: char) -> Option<usize> {
    chars[from..].iter().position(|&c| c == target).map(|p| from + p)
}

/// `http://` / `https://` から URL 文字が続く範囲を除去する。
/// URL に使えるのは ASCII の一部のみなので、日本語文字が来たらそこで終わる。
fn strip_urls(s: &str) -> String {
    let mut out = String::new();
    let mut rest = s;
    loop {
        let pos = match (rest.find("http://"), rest.find("https://")) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        };
        let Some(pos) = pos else {
            out.push_str(rest);
            return out;
        };
        out.push_str(&rest[..pos]);
        let after = &rest[pos..];
        let end = after
            .char_indices()
            .find(|(_, c)| !is_url_char(*c))
            .map(|(i, _)| i)
            .unwrap_or(after.len());
        rest = &after[end..];
    }
}

fn is_url_char(c: char) -> bool {
    c.is_ascii_alphanumeric()
        || matches!(
            c,
            '-' | '.' | '_' | '~' | ':' | '/' | '?' | '#' | '[' | ']' | '@' | '!' | '$' | '&'
                | '\'' | '(' | ')' | '+' | ',' | ';' | '=' | '%'
        )
}

// ---- 構造除去（HTML） ----

/// HTML を平文へ。`<script>`/`<style>`/`<pre>` は中身ごと捨て、その他のタグは
/// 剥がす。`<code>` はタグだけ剥がして中身を残す — Zenn はインラインコードも
/// `<code>` で囲むため、中身ごと消すと「 や  によって」のような穴あき文が
/// 生まれる（実記事の dry-run 目視で確認）。コードブロックは `<pre>` 側が
/// 丸ごと消すので影響しない。Markdown のバッククォート扱いとも対称になる。
/// ブロック終了タグと `<br>` は改行扱い（文分割の助け）。
fn strip_html(s: &str) -> String {
    let mut out = String::new();
    let mut rest = s;
    while let Some(lt) = rest.find('<') {
        out.push_str(&rest[..lt]);
        rest = &rest[lt..];
        let Some(gt) = rest.find('>') else {
            // 閉じない '<'。以降はタグとして不成立なのでそのまま出して終わり。
            out.push_str(rest);
            rest = "";
            break;
        };
        let tag = &rest[1..gt];
        let is_closing = tag.starts_with('/');
        let name: String = tag
            .trim_start_matches('/')
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric())
            .collect::<String>()
            .to_ascii_lowercase();
        rest = &rest[gt + 1..];

        match name.as_str() {
            // 読み上げ不能ブロック: 対応する閉じタグまで中身ごと飛ばす。
            "script" | "style" | "pre" if !is_closing => {
                let close = format!("</{name}");
                if let Some(p) = find_ascii_ci(rest, &close) {
                    let after = &rest[p..];
                    if let Some(g) = after.find('>') {
                        rest = &after[g + 1..];
                    } else {
                        rest = "";
                    }
                } else {
                    rest = "";
                }
            }
            // ブロック境界は改行に（インラインタグで文を割らないため限定列挙）。
            "p" | "div" | "li" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6" | "tr" | "table"
                if is_closing =>
            {
                out.push('\n');
            }
            "br" => out.push('\n'),
            _ => {}
        }
    }
    out.push_str(rest);
    decode_entities(&out)
}

/// ASCII 限定の case-insensitive 検索。needle は ASCII 小文字で渡す。
/// UTF-8 のマルチバイト列に ASCII バイトは現れないため、バイト走査で安全。
fn find_ascii_ci(haystack: &str, needle_lower: &str) -> Option<usize> {
    let h = haystack.as_bytes();
    let n = needle_lower.as_bytes();
    if n.is_empty() || h.len() < n.len() {
        return None;
    }
    'outer: for i in 0..=(h.len() - n.len()) {
        for j in 0..n.len() {
            if h[i + j].to_ascii_lowercase() != n[j] {
                continue 'outer;
            }
        }
        return Some(i);
    }
    None
}

/// 最低限の HTML エンティティ。`&amp;` は最後（二重デコードを避ける）。
fn decode_entities(s: &str) -> String {
    s.replace("&nbsp;", " ")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&amp;", "&")
}

// ---- 文分割・フィルタ・スコア ----

/// 。！？ と改行で切る。区切り記号は文に含めない（TTS へは句点なしで渡る。
/// harvest 側の diff 前処理とも整合し、句読点が表記 diff に混ざらない）。
fn split_sentences(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    for line in s.lines() {
        for ch in line.chars() {
            if matches!(ch, '。' | '！' | '？') {
                flush_sentence(&mut out, &mut cur);
            } else {
                cur.push(ch);
            }
        }
        flush_sentence(&mut out, &mut cur);
    }
    out
}

fn flush_sentence(out: &mut Vec<String>, cur: &mut String) {
    let t = cur.trim();
    if !t.is_empty() {
        out.push(t.to_string());
    }
    cur.clear();
}

fn passes_filters(s: &str, opts: &ExtractOptions) -> bool {
    let len = s.chars().count();
    if len < opts.min_len || len > opts.max_len {
        return false;
    }
    if japanese_ratio(s) < opts.min_japanese_ratio {
        return false;
    }
    if opts.check_residue && has_residue(s) {
        return false;
    }
    true
}

/// 構造除去をすり抜けた残骸を含む文は丸ごと捨てる（迷ったら捨てる）。
/// 対象: コード・表・タグ・数式・URL の痕跡に典型的な文字、裸の角括弧
/// （`[追記]` など。リンク処理後に残るのは構造物）、連続ピリオド、
/// 閉じの合わない括弧（文分割で切れた断片）。
/// 半角の `.`/`,`/`(`/`)` 単体は自然文にも現れるので対象にしない。
fn has_residue(s: &str) -> bool {
    if s.contains("http") || s.contains("..") {
        return true;
    }
    if s.chars().any(|c| {
        matches!(
            c,
            '|' | '`' | '<' | '>' | '{' | '}' | '\\' | '=' | '#' | '$' | ';' | '_' | '~'
                | '[' | ']'
        )
    }) {
        return true;
    }
    has_unbalanced_brackets(s)
}

/// 括弧（半角丸・全角丸・かぎ・二重かぎ）の開閉が釣り合わない文は、
/// 文分割で切れた断片とみなして捨てる。
fn has_unbalanced_brackets(s: &str) -> bool {
    let pairs = [('(', ')'), ('（', '）'), ('「', '」'), ('『', '』')];
    for (open, close) in pairs {
        let mut depth = 0i32;
        for c in s.chars() {
            if c == open {
                depth += 1;
            } else if c == close {
                depth -= 1;
                if depth < 0 {
                    return true;
                }
            }
        }
        if depth != 0 {
            return true;
        }
    }
    false
}

/// 日本語文字（ひらがな・カタカナ・漢字・長音・々）率。分母は空白以外の全文字。
fn japanese_ratio(s: &str) -> f64 {
    ratio_of(s, is_japanese_char)
}

/// 漢字率（スコアリング用）。
fn kanji_ratio(s: &str) -> f64 {
    ratio_of(s, is_kanji)
}

fn ratio_of(s: &str, pred: fn(char) -> bool) -> f64 {
    let mut total = 0usize;
    let mut hit = 0usize;
    for c in s.chars() {
        if c.is_whitespace() {
            continue;
        }
        total += 1;
        if pred(c) {
            hit += 1;
        }
    }
    if total == 0 {
        return 0.0;
    }
    hit as f64 / total as f64
}

fn is_japanese_char(c: char) -> bool {
    matches!(c,
        '\u{3040}'..='\u{309F}' |   // ひらがな
        '\u{30A0}'..='\u{30FF}' |   // カタカナ（長音 U+30FC を含む）
        '\u{4E00}'..='\u{9FFF}' |   // CJK 統合漢字
        '\u{3005}'                  // 々
    )
}

fn is_kanji(c: char) -> bool {
    matches!(c, '\u{4E00}'..='\u{9FFF}' | '\u{3005}')
}

#[cfg(test)]
mod tests {
    use super::*;

    fn md(s: &str) -> Body {
        Body::Markdown(s.to_string())
    }

    fn html(s: &str) -> Body {
        Body::Html(s.to_string())
    }

    fn default_extract(body: &Body) -> Vec<String> {
        extract(body, &ExtractOptions::default())
    }

    // 20 字以上の自然な日本語テスト文（フィルタを通る素材）。
    const SENT_A: &str = "機械学習で意思決定を支援する仕組みを実装する";
    const SENT_B: &str = "要件と用件を整理してから設計に移行する作業だ";

    #[test]
    fn code_fence_contents_are_dropped() {
        let body = md(&format!(
            "{SENT_A}。\n```rust\nこの行はコードフェンスの中にある二十文字以上の日本語文です。\n```\n{SENT_B}。"
        ));
        let got = default_extract(&body);
        assert_eq!(got.len(), 2);
        assert!(got.contains(&SENT_A.to_string()));
        assert!(got.contains(&SENT_B.to_string()));
    }

    #[test]
    fn inline_code_backticks_are_stripped_but_sentence_survives() {
        let body = md(&format!("この`実装`は同音の`衝突`を起こしやすい表現を含む文です。"));
        let got = default_extract(&body);
        assert_eq!(got, vec!["この実装は同音の衝突を起こしやすい表現を含む文です"]);
    }

    #[test]
    fn urls_are_removed_and_clean_sentence_survives() {
        // URL は除去され、残りが自然文として通る。
        let body = md(&format!("詳細は https://example.com/path?q=1 を参照しつつ設計の検討を進める必要がある。"));
        let got = default_extract(&body);
        assert_eq!(got, vec!["詳細は  を参照しつつ設計の検討を進める必要がある"]);
    }

    #[test]
    fn links_keep_text_and_images_drop() {
        let body = md(&format!(
            "[公式の設計資料](https://example.com/doc)を読んでから方針を決定するのが安全だ。\n![スクリーンショット](https://example.com/img.png)\n{SENT_A}。"
        ));
        let got = default_extract(&body);
        assert!(got.contains(&"公式の設計資料を読んでから方針を決定するのが安全だ".to_string()));
        assert!(got.contains(&SENT_A.to_string()));
        assert_eq!(got.len(), 2);
    }

    #[test]
    fn table_rows_are_dropped() {
        let body = md(&format!(
            "| 項目 | 値が二十文字以上になるようにした表のセル |\n|---|---|\n{SENT_A}。"
        ));
        let got = default_extract(&body);
        assert_eq!(got, vec![SENT_A.to_string()]);
    }

    #[test]
    fn heading_and_quote_markers_are_stripped() {
        let body = md(&format!("## {SENT_A}。\n> {SENT_B}。"));
        let got = default_extract(&body);
        assert!(got.contains(&SENT_A.to_string()));
        assert!(got.contains(&SENT_B.to_string()));
    }

    #[test]
    fn list_markers_are_stripped() {
        let body = md(&format!("- {SENT_A}。\n1. {SENT_B}。"));
        let got = default_extract(&body);
        assert!(got.contains(&SENT_A.to_string()));
        assert!(got.contains(&SENT_B.to_string()));
    }

    #[test]
    fn html_tags_are_stripped_and_text_survives() {
        let body = html(&format!("<p>{SENT_A}。</p><p><strong>{SENT_B}</strong>。</p>"));
        let got = default_extract(&body);
        assert!(got.contains(&SENT_A.to_string()));
        assert!(got.contains(&SENT_B.to_string()));
    }

    #[test]
    fn html_code_and_pre_blocks_drop_contents() {
        let body = html(&format!(
            "<pre><code>コードブロックの中の二十文字以上ある日本語の説明文です。</code></pre><p>{SENT_A}。</p>"
        ));
        let got = default_extract(&body);
        assert_eq!(got, vec![SENT_A.to_string()]);
    }

    #[test]
    fn html_inline_code_keeps_contents() {
        // Zenn はインラインコードも <code>。中身を残さないと穴あき文になる。
        let body = html("<p>失敗は<code>Result</code>型で明示し欠損は<code>Option</code>型で表す設計を選ぶ。</p>");
        let got = default_extract(&body);
        assert_eq!(
            got,
            vec!["失敗はResult型で明示し欠損はOption型で表す設計を選ぶ"]
        );
    }

    #[test]
    fn html_entities_are_decoded() {
        // &amp; → & は残骸フィルタに引っかからず、文として残る。
        let body = html(&format!("<p>研究開発&amp;設計部門で要件の整理を進めることが決定した。</p>"));
        let got = default_extract(&body);
        assert_eq!(got, vec!["研究開発&設計部門で要件の整理を進めることが決定した"]);
    }

    #[test]
    fn sentences_split_on_terminators() {
        let body = md(&format!("{SENT_A}。{SENT_B}？"));
        let got = default_extract(&body);
        assert_eq!(got.len(), 2);
    }

    #[test]
    fn length_filter_boundaries() {
        let s19 = "あ".repeat(19);
        let s20 = "あ".repeat(20);
        let s80 = "い".repeat(80);
        let s81 = "う".repeat(81);
        let body = md(&format!("{s19}。\n{s20}。\n{s80}。\n{s81}。"));
        let got = default_extract(&body);
        assert!(got.contains(&s20));
        assert!(got.contains(&s80));
        assert!(!got.contains(&s19));
        assert!(!got.contains(&s81));
    }

    #[test]
    fn japanese_ratio_filter_drops_english_heavy_lines() {
        // 半分超が ASCII 英字 → 落ちる。
        let body = md("これはほぼ english words only sentence with some 日本語です。");
        let got = default_extract(&body);
        assert!(got.is_empty());
    }

    #[test]
    fn residue_filter_drops_leftover_symbols() {
        let body = md(&format!(
            "環境変数 PATH=value を設定してから実行する必要がある。\n{SENT_A}。"
        ));
        let got = default_extract(&body);
        // '=' を含む文は捨てる。
        assert_eq!(got, vec![SENT_A.to_string()]);
    }

    #[test]
    fn residue_filter_drops_real_world_garbage() {
        // Qiita 実記事の dry-run 目視で見つかったゴミ 3 種（2026-06-10）。
        let bare_bracket = "[追記] 緑の位置は本質ではなくてこの後の計算と方向が違うから";
        let unbalanced = "この時点で技術者的には「なにそれ面白そうと感じたという話";
        let double_dot = "正直なところ毎回この設定を書くのは面倒に感じています..";
        let body = md(&format!("{bare_bracket}。\n{unbalanced}。\n{double_dot}。\n{SENT_A}。"));
        let got = default_extract(&body);
        assert_eq!(got, vec![SENT_A.to_string()]);
    }

    #[test]
    fn balanced_brackets_survive() {
        let s = "設計の検討（特に同音異義語の扱い）を進めることが「重要」だと考えた";
        let body = md(&format!("{s}。"));
        assert_eq!(default_extract(&body), vec![s.to_string()]);
    }

    #[test]
    fn cap_keeps_kanji_dense_sentences_and_preserves_order() {
        // 上限(20)超え: 同率のひらがな主体文 20 + 漢字率の高い文 1（最後に出現）。
        // 漢字文は最後尾の出現でも生き残り、同率群の末尾 1 文が落ちる。
        // 出力は選抜後も出現順。
        let mut text = String::new();
        for i in 0..20 {
            text.push_str(&format!(
                "これはひらがなが多めのながいぶんしょう{i:02}番です。\n"
            ));
        }
        text.push_str(&format!("{SENT_A}。\n"));
        let got = default_extract(&md(&text));
        assert_eq!(got.len(), 20);
        // 漢字率の高い文が生き残り、出現順なので最後に来る。
        assert_eq!(got.last().unwrap(), SENT_A);
        // 同率（出現順優先）の末尾、19 番の文が落ちる。
        assert!(!got.iter().any(|s| s.contains("19番")));
    }

    #[test]
    fn under_cap_keeps_document_order() {
        // 上限未満なら選抜もソートもせず、記事の出現順のまま。
        let hira = "これはひらがなだけでできているながいぶんしょうです";
        let body = md(&format!("{hira}。\n{SENT_A}。"));
        let got = default_extract(&body);
        assert_eq!(got, vec![hira.to_string(), SENT_A.to_string()]);
    }

    #[test]
    fn duplicates_within_article_are_removed() {
        let body = md(&format!("{SENT_A}。\n{SENT_A} 。\n{SENT_B}。"));
        let got = default_extract(&body);
        // 空白の有無は正規化で同一視される。
        assert_eq!(got.len(), 2);
    }

    #[test]
    fn normalize_sentence_strips_whitespace() {
        assert_eq!(
            normalize_sentence("機械 学習　で\t実装"),
            "機械学習で実装"
        );
    }

    #[test]
    fn plain_body_passes_through_filters_only() {
        let body = Body::Plain(format!("{SENT_A}\nshort"));
        let got = default_extract(&body);
        assert_eq!(got, vec![SENT_A.to_string()]);
    }

    #[test]
    fn lenient_profile_keeps_short_and_symbol_sentences() {
        // 明示入力: 19 字以下も記号入りも捨てない（重複排除だけ効く）。
        let short = "確率的な基盤を確立する";
        let symbols = "PATH=value を設定する";
        let body = Body::Plain(format!("{short}\n{symbols}\n{short}"));
        let got = extract(&body, &ExtractOptions::lenient());
        assert_eq!(got, vec![short.to_string(), symbols.to_string()]);
    }
}

//! UI メッセージの日英切り替え（最小実装）。
//!
//! 環境変数 `BIASDIFF_LANG` / `LC_ALL` / `LANG` を見て、`ja` で始まれば日本語、
//! それ以外は英語。CLI 文字列をハードコードせず1か所のマクロに集約しておくことで、
//! 後から resource bundle へ移しやすくしておく。

use std::sync::OnceLock;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    En,
    Ja,
}

/// 実行環境の言語を一度だけ判定して返す。
pub fn lang() -> Lang {
    static LANG: OnceLock<Lang> = OnceLock::new();
    *LANG.get_or_init(|| {
        let v = std::env::var("BIASDIFF_LANG")
            .or_else(|_| std::env::var("LC_ALL"))
            .or_else(|_| std::env::var("LANG"))
            .unwrap_or_default()
            .to_ascii_lowercase();
        if v.starts_with("ja") {
            Lang::Ja
        } else {
            Lang::En
        }
    })
}

/// `msg!(英語, 日本語)` で実行環境に応じた文字列を選ぶ。
///
/// 両アームは同じ型である必要がある（ともに `&str`、またはともに `String`）。
macro_rules! msg {
    ($en:expr, $ja:expr $(,)?) => {
        match $crate::messages::lang() {
            $crate::messages::Lang::En => $en,
            $crate::messages::Lang::Ja => $ja,
        }
    };
}

pub(crate) use msg;

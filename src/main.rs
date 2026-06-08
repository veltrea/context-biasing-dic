//! biasdiff: コンテキストバイアシング辞書のための diff 照合ユーティリティ。
//!
//! 実体は `cli::run`。辞書を埋め込まずにビルドした場合は、その旨を伝えて終了する。

#[cfg(feature = "_lindera")]
fn main() -> anyhow::Result<()> {
    biasdiff::cli::run()
}

#[cfg(not(feature = "_lindera"))]
fn main() {
    eprintln!(
        "biasdiff was built without an embedded dictionary.\n\
         Rebuild with a dictionary feature, e.g.:\n  \
         cargo build --release --features ipadic   (lightweight)\n  \
         cargo build --release --no-default-features --features unidic   (higher reading accuracy)"
    );
    std::process::exit(2);
}

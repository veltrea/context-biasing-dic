#!/usr/bin/env bash
#
# biasdiff installer
#
# Builds biasdiff and installs the binary into a bin directory on your PATH.
#
# Usage:
#   ./install.sh                 # IPADIC build (lightweight), auto-pick prefix
#   ./install.sh --unidic        # UniDic build (higher reading accuracy, heavy)
#   ./install.sh --prefix DIR    # install into DIR (e.g. /usr/local/bin)
#   ./install.sh --help
#
set -euo pipefail

dict_feature="ipadic"
prefix=""

usage() {
  sed -n '2,12p' "$0" | sed 's/^# \{0,1\}//'
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --ipadic) dict_feature="ipadic"; shift ;;
    --unidic) dict_feature="unidic"; shift ;;
    --prefix) prefix="${2:?--prefix needs a directory}"; shift 2 ;;
    -h|--help) usage; exit 0 ;;
    *) echo "unknown option: $1" >&2; usage >&2; exit 1 ;;
  esac
done

# Need cargo to build.
if ! command -v cargo >/dev/null 2>&1; then
  echo "error: cargo not found. Install Rust from https://rustup.rs" >&2
  exit 1
fi

# Pick an install prefix if none given: prefer /usr/local/bin when writable,
# otherwise fall back to ~/.local/bin (no sudo needed).
if [[ -z "$prefix" ]]; then
  if [[ -w /usr/local/bin ]]; then
    prefix="/usr/local/bin"
  else
    prefix="$HOME/.local/bin"
  fi
fi
mkdir -p "$prefix"

# Build from the script's own directory (the repo root).
script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$script_dir"

echo "Building biasdiff ($dict_feature) in release mode..."
if [[ "$dict_feature" == "unidic" ]]; then
  cargo build --release --no-default-features --features unidic
else
  cargo build --release --features ipadic
fi

install -m 0755 "target/release/biasdiff" "$prefix/biasdiff"
echo "Installed: $prefix/biasdiff"

# Warn if the prefix is not on PATH.
case ":$PATH:" in
  *":$prefix:"*) ;;
  *)
    echo
    echo "note: $prefix is not on your PATH. Add this to your shell profile:"
    echo "  export PATH=\"$prefix:\$PATH\""
    ;;
esac

echo
echo "Try it:"
echo "  biasdiff --help"
echo "  biasdiff repl"

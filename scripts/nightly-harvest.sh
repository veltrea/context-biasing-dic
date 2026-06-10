#!/bin/bash
# nightly-harvest.sh — 夜間の自動収穫・評価（実装計画書 Step 5）。
#
# Qiita / Zenn から収穫 → 当日分の辞書をマージ → evaluate でカーブと
# 剪定済み辞書を出し、成果物を nightly/{日付}/ に残す。キャッシュと
# seen.jsonl の働きで、毎晩の実行は新しい記事・新しい文の分だけ進む。
#
# マシン固有の宛先・鍵・電源情報はすべて環境変数で注入する — リポジトリには
# この汎用スクリプトだけを置く。BIASDIFF_REMOTE_HOST 未設定ならローカル実行
# なので、手動の動作確認にも同じスクリプトを使える。
#
#   BIASDIFF_REMOTE_HOST  SSH 宛先（user@host）。未設定 = ローカル実行
#   BIASDIFF_REMOTE_DIR   リモートのリポジトリパス（リモート実行時に必須）
#   BIASDIFF_REMOTE_MAC   Wake-on-LAN の MAC アドレス（未設定 = WoL しない）
#   BIASDIFF_SSH_OPTS     追加 SSH オプション（例: "-o ProxyJump=none -i ~/.ssh/id_ed25519"）
#   BIASDIFF_SHUTDOWN     "1" で実行後にリモートを shutdown -h（パスワードレス
#                         sudo が前提。設定はマシン側の責務）
#   BIASDIFF_FETCH_TO     リモートの成果物を取り込むローカルディレクトリ
#                         （未設定 = 取り込まない）
#
#   BIASDIFF_VENV         mlx-audio venv（既定: ~/.venvs/mlx-audio）
#   BIASDIFF_QIITA_QUERY  Qiita クエリ（既定: "stocks:>=50 tag:rust"）
#   BIASDIFF_ZENN_TOPIC   Zenn トピック（既定: rust）
#   BIASDIFF_COUNT        ソースごとの記事数（既定: 30）
#   BIASDIFF_VOICES       VOICEVOX 話者（既定: 3,2,11）
#   BIASDIFF_CACHE_DIR    キャッシュ（既定: ./harvest_cache）
#   BIASDIFF_OUT_ROOT     成果物ルート（既定: ./nightly）
#   BIASDIFF_MAX_WORDS    evaluate の N 上限（既定: 200）
#   BIASDIFF_STEP         evaluate の N 刻み（既定: 25）
set -euo pipefail

# SSH 経由の non-login shell では Homebrew の PATH が通っていないことがある
# （ffmpeg・uv が見つからず ASR 前段で全滅する）。スクリプト自身で自衛する。
export PATH="/opt/homebrew/bin:/usr/local/bin:$PATH"

REMOTE_HOST="${BIASDIFF_REMOTE_HOST:-}"
REMOTE_DIR="${BIASDIFF_REMOTE_DIR:-}"
REMOTE_MAC="${BIASDIFF_REMOTE_MAC:-}"
SSH_OPTS="${BIASDIFF_SSH_OPTS:-}"
SHUTDOWN="${BIASDIFF_SHUTDOWN:-0}"
FETCH_TO="${BIASDIFF_FETCH_TO:-}"

VENV="${BIASDIFF_VENV:-$HOME/.venvs/mlx-audio}"
QIITA_QUERY="${BIASDIFF_QIITA_QUERY:-stocks:>=50 tag:rust}"
ZENN_TOPIC="${BIASDIFF_ZENN_TOPIC:-rust}"
COUNT="${BIASDIFF_COUNT:-30}"
VOICES="${BIASDIFF_VOICES:-3,2,11}"
CACHE_DIR="${BIASDIFF_CACHE_DIR:-./harvest_cache}"
OUT_ROOT="${BIASDIFF_OUT_ROOT:-./nightly}"
MAX_WORDS="${BIASDIFF_MAX_WORDS:-200}"
STEP="${BIASDIFF_STEP:-25}"

DATE_TAG="$(date +%F)"

log() { echo "[nightly $(date +%T)] $*" >&2; }

# ---- リモートモード: WoL → SSH 待ち → リモートでローカルモードを再帰実行 ----
if [ -n "$REMOTE_HOST" ]; then
    [ -n "$REMOTE_DIR" ] || { echo "BIASDIFF_REMOTE_DIR is required with BIASDIFF_REMOTE_HOST" >&2; exit 2; }

    if [ -n "$REMOTE_MAC" ]; then
        log "waking $REMOTE_MAC"
        wakeonlan "$REMOTE_MAC" >/dev/null
    fi

    log "waiting for ssh on $REMOTE_HOST"
    tries=0
    # shellcheck disable=SC2086
    until ssh $SSH_OPTS -o ConnectTimeout=5 -o BatchMode=yes "$REMOTE_HOST" true 2>/dev/null; do
        tries=$((tries + 1))
        if [ "$tries" -ge 36 ]; then   # 最大 3 分待つ
            echo "remote $REMOTE_HOST did not come up" >&2
            exit 1
        fi
        sleep 5
    done

    log "running nightly harvest on $REMOTE_HOST"
    # リモート側では同じスクリプトがローカルモードで走る。収穫パラメータを
    # 環境変数ごと運ぶ（REMOTE_* は落とすのでループしない）。
    # BIASDIFF_VENV はパスを含むため**明示されたときだけ**運ぶ — 既定のまま
    # 運ぶと、ローカルの $HOME で展開された venv パスがリモートへ漏れて
    # 存在しない venv を探し、ASR が全滅する（実際に起きた）。
    VENV_FORWARD=""
    if [ -n "${BIASDIFF_VENV:-}" ]; then
        VENV_FORWARD="BIASDIFF_VENV='$BIASDIFF_VENV'"
    fi
    # shellcheck disable=SC2086
    ssh $SSH_OPTS "$REMOTE_HOST" \
        "cd '$REMOTE_DIR' && \
         $VENV_FORWARD \
         BIASDIFF_QIITA_QUERY='$QIITA_QUERY' \
         BIASDIFF_ZENN_TOPIC='$ZENN_TOPIC' \
         BIASDIFF_COUNT='$COUNT' \
         BIASDIFF_VOICES='$VOICES' \
         BIASDIFF_CACHE_DIR='$CACHE_DIR' \
         BIASDIFF_OUT_ROOT='$OUT_ROOT' \
         BIASDIFF_MAX_WORDS='$MAX_WORDS' \
         BIASDIFF_STEP='$STEP' \
         ./scripts/nightly-harvest.sh"

    if [ -n "$FETCH_TO" ]; then
        mkdir -p "$FETCH_TO/$DATE_TAG"
        log "fetching artifacts to $FETCH_TO/$DATE_TAG"
        # shellcheck disable=SC2086
        scp $SSH_OPTS -r "$REMOTE_HOST:$REMOTE_DIR/$OUT_ROOT/$DATE_TAG/." "$FETCH_TO/$DATE_TAG/"
    fi

    if [ "$SHUTDOWN" = "1" ]; then
        log "shutting down $REMOTE_HOST"
        # shellcheck disable=SC2086
        ssh $SSH_OPTS "$REMOTE_HOST" "sudo shutdown -h now" || true
    fi
    log "remote nightly done"
    exit 0
fi

# ---- ローカルモード: 収穫 → マージ → 評価 ----
OUT_DIR="$OUT_ROOT/$DATE_TAG"
mkdir -p "$OUT_DIR"

# ブート直後の実行（pmset 自己起床 + launchd）では VOICEVOX エンジンの
# LaunchDaemon がまだ初期化中のことがある。立ち上がりを待ってから収穫する
# （最大 2 分。来なければ警告して続行 — say フォールバック等の構成もあるため）。
VOICEVOX_URL="${BIASDIFF_VOICEVOX_URL:-http://127.0.0.1:50021}"
vv_tries=0
until curl -s --max-time 2 "$VOICEVOX_URL/version" >/dev/null 2>&1; do
    vv_tries=$((vv_tries + 1))
    if [ "$vv_tries" -ge 24 ]; then
        log "warning: VOICEVOX engine at $VOICEVOX_URL not reachable after ~2min"
        break
    fi
    [ "$vv_tries" -eq 1 ] && log "waiting for VOICEVOX engine at $VOICEVOX_URL"
    sleep 5
done

# venv を有効化（python3 = mlx-audio 入りの環境にする。ASR ドライバの前提）。
if [ -f "$VENV/bin/activate" ]; then
    # shellcheck disable=SC1091
    source "$VENV/bin/activate"
else
    log "warning: venv $VENV not found; relying on PATH python3"
fi

BIN=./target/release/biasdiff
if [ ! -x "$BIN" ]; then
    log "building biasdiff (release, --features harvest)"
    cargo build --release --features harvest
fi

harvest_one() {
    # $1=ソース名 $2... = ソース固有引数
    local name="$1"; shift
    log "harvest: $name"
    if ! "$BIN" harvest "$@" \
        --tts voicevox --voices "$VOICES" \
        --asr qwen3-mlx \
        --cache-dir "$CACHE_DIR" \
        --format counts \
        -o "$OUT_DIR/$name.counts.tsv" \
        --reject "$OUT_DIR/$name.reject.tsv" \
        2> "$OUT_DIR/$name.log"; then
        # 片方のソースの失敗（API 変更・レート制限）で夜間全体を止めない。
        log "warning: $name harvest failed (see $OUT_DIR/$name.log)"
        : > "$OUT_DIR/$name.counts.tsv"
    fi
}

harvest_one qiita --source qiita --query "$QIITA_QUERY" --count "$COUNT"
harvest_one zenn --source zenn --topic "$ZENN_TOPIC" --order weekly --count "$COUNT"

# 当日の counts をマージ（語ごとに合算 → count 降順・同数は語順）。
MERGED="$OUT_DIR/dict.counts.tsv"
awk -F'\t' 'NF==2 {c[$1]+=$2} END {for (w in c) printf "%s\t%d\n", w, c[w]}' \
    "$OUT_DIR/qiita.counts.tsv" "$OUT_DIR/zenn.counts.tsv" \
    | sort -t$'\t' -k2,2nr -k1,1 > "$MERGED"
log "merged dictionary: $(wc -l < "$MERGED" | tr -d ' ') word(s)"

# 評価: 当日の辞書をキャッシュ済み音声セットへ biasing 投入し、頭打ちを探す。
if [ -s "$MERGED" ]; then
    log "evaluate"
    if ! "$BIN" evaluate \
        --input "$MERGED" \
        --cache-dir "$CACHE_DIR" \
        --step "$STEP" --max-words "$MAX_WORDS" \
        --report "$OUT_DIR/curve.tsv" \
        --prune "$OUT_DIR/pruned.txt" \
        2> "$OUT_DIR/evaluate.log"; then
        log "warning: evaluate failed (see $OUT_DIR/evaluate.log)"
    fi
else
    log "no words harvested today; skipping evaluate"
fi

log "done: artifacts in $OUT_DIR"
ls -la "$OUT_DIR" >&2

# 自律運用（pmset 自己起床 + launchd のローカル実行）では、収穫を終えたら
# 自分で電源状態を下げる（電力節約）。リモートモードの再帰実行にはこの変数は
# 転送されないため、取り込み（scp）前にリモートが落ちることはない。
#
#   BIASDIFF_SHUTDOWN=sleep — スリープ（推奨）。Apple Silicon Mac は
#       **shutdown 状態からは WoL で起こせない**（スリープからのみ）ため、
#       日中にオンデマンドで起こしたいならスリープ一択。消費は 1W 未満。
#   BIASDIFF_SHUTDOWN=1     — 完全シャットダウン。次の起動は pmset の
#       電源オンスケジュール（wakeorpoweron）か物理ボタンのみ。
case "$SHUTDOWN" in
    sleep)
        log "putting this machine to sleep"
        pmset sleepnow || log "warning: sleepnow failed"
        ;;
    1 | shutdown)
        log "shutting down this machine"
        sudo -n /sbin/shutdown -h now || log "warning: self-shutdown failed (sudoers?)"
        ;;
esac

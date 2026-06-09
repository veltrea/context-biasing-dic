"""Qwen3-ASR batch driver (SPEC sec. 9).

Loads the model once, then converses over JSONL:

    stdin:  {"id": "...", "audio": "/path/x.wav", "bias": ["機械", "意思"]}   # bias may be null
    stdout: {"id": "...", "ok": true,  "text": "..."}
            {"id": "...", "ok": false, "error": "..."}

Why this exists (both reasons matter):
  1. Model load dominates per-process cost (~2 s per sentence measured in
     Step 0); loading once makes long harvests practical.
  2. Context biasing only reaches Qwen3-ASR through the Python API's
     `system_prompt=` parameter. The mlx-audio 0.4.4 CLI accepts `--context`
     but filters kwargs by `inspect.signature(model.generate)`, and the
     Qwen3 signature has no `context` parameter, so the flag is silently
     dropped (verified by reading the installed source). Bias words are
     joined with single spaces, matching the official Qwen3-ASR examples
     (e.g. context="交易 停滞"); no preamble.

The Rust adapter embeds this file at compile time and materializes it to a
temp path at runtime, so the binary stays self-contained.
"""

import argparse
import json
import sys


def main() -> None:
    parser = argparse.ArgumentParser(description="Qwen3-ASR JSONL batch driver")
    parser.add_argument("--model", required=True, help="model repo or local path")
    args = parser.parse_args()

    # Import after argparse so `--help` works without the venv being complete.
    from mlx_audio.stt.utils import load_model

    model = load_model(args.model)
    # Readiness handshake: the parent waits for this line before sending work.
    print(json.dumps({"ready": True, "model": args.model}), flush=True)

    for line in sys.stdin:
        line = line.strip()
        if not line:
            continue
        req_id = None
        try:
            req = json.loads(line)
            req_id = req.get("id")
            bias = req.get("bias")
            system_prompt = " ".join(bias) if bias else None
            result = model.generate(
                req["audio"], system_prompt=system_prompt, verbose=False
            )
            print(
                json.dumps(
                    {"id": req_id, "ok": True, "text": result.text},
                    ensure_ascii=False,
                ),
                flush=True,
            )
        except Exception as e:  # noqa: BLE001 - report and keep serving
            print(
                json.dumps(
                    {"id": req_id, "ok": False, "error": f"{type(e).__name__}: {e}"},
                    ensure_ascii=False,
                ),
                flush=True,
            )


if __name__ == "__main__":
    main()

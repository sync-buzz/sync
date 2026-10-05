#!/usr/bin/env python3
"""Laya sidecar: loads the model once and answers JSON-RPC over stdio.

Speaks the same protocol as `sync-inference-server` — line-delimited JSON,
one request per line in, one response per line out — so the Rust server can
forward requests to this script without translating.

The model is loaded lazily on the first ``run`` request and held for the
process's life. A ``run`` that names a different HuggingFace repo swaps the
loaded model.

Requires ``pip install laya`` on the machine. The model weights are fetched
and cached by the ``laya`` library itself (via ``huggingface_hub``), so this
script does not download them.
"""

import json
import sys

CHANNEL_VERSION = 1


def main():
    agent = None
    loaded_repo = None

    for line in sys.stdin:
        line = line.strip()
        if not line:
            continue

        try:
            request = json.loads(line)
        except json.JSONDecodeError as error:
            _respond(
                {
                    "error": {"code": "bad_request", "message": str(error)},
                    "result": None,
                }
            )
            continue

        method = request.get("method", "")
        params = request.get("params", {})

        if method == "handshake":
            _respond(
                {
                    "error": None,
                    "result": {
                        "channel": CHANNEL_VERSION,
                        "version": "0.1.0",
                        "runtimes": ["laya"],
                    },
                }
            )

        elif method == "run":
            model_path = params.get("modelPath", "")

            # Load or swap the model.
            if agent is None or model_path != loaded_repo:
                try:
                    import laya

                    loaded_repo = model_path
                    agent = laya.load(model_path)
                except Exception as error:
                    _respond(
                        {
                            "error": {"code": "load_failed", "message": str(error)},
                            "result": None,
                        }
                    )
                    continue

            # Run the prediction. The input carries `state` and `questions`
            # in the shape Laya's API expects — the handler constructs them.
            try:
                input_data = params.get("input", {})
                state = input_data.get("state", {})
                questions = input_data.get("questions", {})

                if not questions:
                    # A test prompt with no structured questions: return
                    # routing metadata so the caller can see the model is alive.
                    result = {"routing": {"model": loaded_repo}, "answers": {}}
                else:
                    result = agent.predict(state, questions)

                _respond({"error": None, "result": result})
            except Exception as error:
                _respond(
                    {
                        "error": {"code": "runtime_failed", "message": str(error)},
                        "result": None,
                    }
                )

        else:
            _respond(
                {
                    "error": {
                        "code": "unsupported",
                        "message": f"unknown method: {method}",
                    },
                    "result": None,
                }
            )


def _respond(response):
    """Write one JSON line to stdout and flush."""
    print(json.dumps(response), flush=True)


if __name__ == "__main__":
    main()

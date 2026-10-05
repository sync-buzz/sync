#!/bin/sh
# prepare-inference-sidecar.sh — build the inference server and put it where
# Tauri bundles it.
#
# Tauri's `externalBin` entries are looked up as `binaries/<name>-<target-triple>`
# and shipped inside the bundle as `<name>`. This script produces that file from
# a release build of `sync-inference-server`.
#
# The inference sidecar links `llama-cpp-2` — the same crate the memory sidecar
# links for embeddings — but not LanceDB, so it builds where `sync-mcp` cannot.
# See `prepare-sidecar.sh` for the parallel script that stages the memory
# sidecar.

set -eu

FROM=""
TARGET=""

usage() {
    cat <<'MESSAGE'
prepare-inference-sidecar.sh — build the inference server and put it where Tauri bundles it.

Usage:
  ./scripts/prepare-inference-sidecar.sh                    # build from this workspace
  ./scripts/prepare-inference-sidecar.sh --from <binary>    # stage an existing binary
  ./scripts/prepare-inference-sidecar.sh --target <triple>  # build and name for another target
MESSAGE
    exit 0
}

while [ $# -gt 0 ]; do
    case "$1" in
        --from) FROM="$2"; shift 2 ;;
        --target) TARGET="$2"; shift 2 ;;
        --help|-h) usage ;;
        *) echo "unknown option: $1" >&2; exit 2 ;;
    esac
done

HOST="$(rustc -vV | awk '/^host:/ {print $2}')"
if [ -z "$TARGET" ]; then
    TARGET="$HOST"
fi

SCRIPT_DIR="$(cd -- "$(dirname -- "$0")" && pwd)"
WORKSPACE="$SCRIPT_DIR/../src-tauri"
DESTINATION_DIR="$WORKSPACE/binaries"

SUFFIX=""
case "$TARGET" in
    *windows*) SUFFIX=".exe" ;;
esac
DESTINATION="$DESTINATION_DIR/sync-inference-server-$TARGET$SUFFIX"

if [ -z "$FROM" ]; then
    if [ "$TARGET" = "$HOST" ]; then
        echo "building sync-inference-server for $TARGET"
        ( cd "$WORKSPACE" && cargo build --release --locked -p sync-inference-server )
        FROM="$WORKSPACE/target/release/sync-inference-server$SUFFIX"
    else
        echo "cross-building sync-inference-server for $TARGET"
        ( cd "$WORKSPACE" && cargo build --release --locked --target "$TARGET" -p sync-inference-server )
        FROM="$WORKSPACE/target/$TARGET/release/sync-inference-server$SUFFIX"
    fi
fi

if [ ! -f "$FROM" ]; then
    echo "no sync-inference-server binary at $FROM" >&2
    exit 1
fi

mkdir -p "$DESTINATION_DIR"
cp "$FROM" "$DESTINATION"
chmod +x "$DESTINATION"

echo "inference sidecar ready: $DESTINATION"

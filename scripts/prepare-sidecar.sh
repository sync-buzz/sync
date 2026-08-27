#!/bin/sh
# prepare-sidecar.sh — build Sync's MCP server and put it where Tauri bundles it.
#
# Tauri's `externalBin` entries are looked up as `binaries/<name>-<target-triple>`
# and shipped inside the bundle as `<name>`. This script produces that file from
# a release build of `sync-mcp`.
#
# The sidecar is a crate of this workspace now, not a foreign artifact: it links
# the memory engine instead of spawning it, so there is no separate engine
# binary to fetch, build or keep in step. The engine comes from the tag its
# manifest pins, so a clean checkout builds what a release is cut against and
# nothing here needs a directory beside it.

set -eu

FROM=""
TARGET=""

usage() {
    cat <<'MESSAGE'
prepare-sidecar.sh — build Sync's MCP server and put it where Tauri bundles it.

Usage:
  ./scripts/prepare-sidecar.sh                    # build from this workspace
  ./scripts/prepare-sidecar.sh --from <binary>    # stage an existing binary
  ./scripts/prepare-sidecar.sh --target <triple>  # build and name for another target
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

# Tauri looks an `externalBin` up as `binaries/<name>-<triple><exe suffix>`, and
# on Windows that suffix is part of the name it looks for — a file without it is
# a bundle that builds and ships nothing to run.
SUFFIX=""
case "$TARGET" in
    *windows*) SUFFIX=".exe" ;;
esac
DESTINATION="$DESTINATION_DIR/sync-mcp-$TARGET$SUFFIX"

if [ -z "$FROM" ]; then
    # `--locked`: the lock file describes this build exactly — the engine is a
    # pinned tag like any other dependency — and a build that quietly resolved
    # something else is a bundle nobody can reproduce.
    if [ "$TARGET" = "$HOST" ]; then
        # No `--target` for the host: passing one sends cargo to a separate
        # target directory, and the sidecar's dependency graph carries LanceDB
        # and llama.cpp — a second full build of those to produce a file the
        # ordinary one already produced.
        echo "building sync-mcp for $TARGET"
        ( cd "$WORKSPACE" && cargo build --release --locked -p sync-mcp )
        FROM="$WORKSPACE/target/release/sync-mcp$SUFFIX"
    else
        echo "cross-building sync-mcp for $TARGET"
        ( cd "$WORKSPACE" && cargo build --release --locked --target "$TARGET" -p sync-mcp )
        FROM="$WORKSPACE/target/$TARGET/release/sync-mcp$SUFFIX"
    fi
fi

if [ ! -f "$FROM" ]; then
    echo "no sync-mcp binary at $FROM" >&2
    exit 1
fi

mkdir -p "$DESTINATION_DIR"
cp "$FROM" "$DESTINATION"
chmod +x "$DESTINATION"

echo "sidecar ready: $DESTINATION"
# Runs what was just staged, not what was just built: a copy that cannot start
# is a bundle that ships a window with no memory, and here it costs one line to
# find out.
"$DESTINATION" --version

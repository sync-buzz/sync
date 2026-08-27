#!/usr/bin/env bash
#
# release.sh — set the product version everywhere and cut a release tag.
#
# The version a person can see lives in `src-tauri/tauri.conf.json`. That is
# the one the bundler stamps into the installer's name and into the bundle the
# updater compares against, so it is the source of truth here; the crate
# version and package.json are mirrors this script keeps in step, and nothing
# reads them.
#
# Usage:
#   scripts/release.sh <version>            # e.g. scripts/release.sh 0.8.1
#   scripts/release.sh <version> --push     # also push branch and tag
#   scripts/release.sh <version> --dry-run  # say what would change, touch nothing
#
# Pushing the tag is what starts the release build, so by default this stops
# after the local commit and tag and prints the command.
#
# Phase 2 — publish the update manifest. Run it AFTER the build for the tag has
# finished. It reads the signatures back off the GitHub release, assembles
# `updater/manifest.json` and commits it. Installed copies poll that file on
# `main`, so pushing the commit is the moment anybody starts receiving the
# version. Two handles, deliberately:
#
#   tag pushed   = built     (the installers exist on the release page)
#   main pushed  = released  (installed copies begin updating to it)
#
#   scripts/release.sh <version> --publish-manifest         # commit, print the push
#   scripts/release.sh <version> --publish-manifest --push  # commit and push = live
#   ... --publish-manifest --dist <dir>      # read the artifacts and signatures from
#                                            # a local directory instead of the release
#   ... --publish-manifest --base-url <url>  # point the manifest's URLs elsewhere
#   ... --publish-manifest --out <path>      # write the manifest only, commit nothing
#
# `updater/latest.json` is not this file and is never written here. It is the
# frozen manifest of the product that used to live at this address, kept so
# that copies of it still installed somewhere get a truthful "nothing new"
# rather than a 404 — see docs/releasing.md.

set -euo pipefail

PUSH=0
DRY_RUN=0
PUBLISH_MANIFEST=0
BASE_URL=""
OUT=""
DIST=""
VERSION=""

for arg in "$@"; do
  case "$arg" in
    --push)             PUSH=1 ;;
    --dry-run)          DRY_RUN=1 ;;
    --publish-manifest) PUBLISH_MANIFEST=1 ;;
    --base-url=*)       BASE_URL="${arg#--base-url=}" ;;
    --out=*)            OUT="${arg#--out=}" ;;
    --dist=*)           DIST="${arg#--dist=}" ;;
    # Two-token forms (--flag value) are handled below through the EXPECT slot.
    --base-url|--out|--dist) EXPECT="${arg#--}" ;;
    -h|--help)
      sed -n '2,40p' "$0" | sed 's/^# \{0,1\}//'
      exit 0
      ;;
    -*) echo "release: unknown flag '$arg'" >&2; exit 2 ;;
    *)
      if [ -n "${EXPECT:-}" ]; then
        case "$EXPECT" in
          base-url) BASE_URL="$arg" ;;
          out)      OUT="$arg" ;;
          dist)     DIST="$arg" ;;
        esac
        EXPECT=""
      elif [ -n "$VERSION" ]; then
        echo "release: unexpected extra argument '$arg'" >&2; exit 2
      else
        VERSION="$arg"
      fi
      ;;
  esac
done
if [ -n "${EXPECT:-}" ]; then
  echo "release: --$EXPECT needs a value" >&2; exit 2
fi

if [ -z "$VERSION" ]; then
  echo "usage: scripts/release.sh <version> [--push] [--dry-run]" >&2
  exit 2
fi

# A leading 'v' is accepted for convenience and normalised away; the tag is
# always 'vX.Y.Z'.
VERSION="${VERSION#v}"

# A semver core, with an optional pre-release or build suffix.
if ! printf '%s' "$VERSION" | grep -Eq '^[0-9]+\.[0-9]+\.[0-9]+([-+][0-9A-Za-z.-]+)?$'; then
  echo "release: '$VERSION' is not a semver version (expected X.Y.Z)" >&2
  exit 2
fi

TAG="v${VERSION}"

REPO_ROOT="$(git rev-parse --show-toplevel)"
cd "$REPO_ROOT"

TAURI_CONF="$REPO_ROOT/src-tauri/tauri.conf.json"
CARGO_TOML="$REPO_ROOT/src-tauri/Cargo.toml"
PKG_JSON="$REPO_ROOT/package.json"

# --- phase 2: assemble and commit the update manifest ------------------------
# Runs INSTEAD of the bump-and-tag flow and touches no manifest: all it needs is
# the released signatures (or --dist) and perl's core JSON::PP.
if [ "$PUBLISH_MANIFEST" -eq 1 ]; then
  # The canonical owner/repo comes from Cargo.toml's `repository`, not from the
  # git remote: a remote may use an ssh host alias that does not parse to the
  # public slug.
  SLUG="$(perl -ne 'if (m{^repository\s*=\s*"https://github\.com/([^"/]+/[^"/]+)"}) { print $1; exit }' "$CARGO_TOML")"
  if [ -z "$SLUG" ]; then
    echo "release: could not derive the github owner/repo from Cargo.toml 'repository'" >&2
    exit 1
  fi

  base="${BASE_URL:-https://github.com/$SLUG/releases/download/$TAG}"
  MANIFEST_OUT="${OUT:-updater/manifest.json}"

  if [ -z "$OUT" ] && ! git diff-index --quiet HEAD --; then
    echo "release: working tree has uncommitted changes — commit or stash first" >&2
    git status --short >&2
    exit 1
  fi

  # The platform key as the updater asks for it, and the stable asset name it
  # gets. Keep these in step with the Collect installers step in release.yml —
  # this list is what the check below refuses over, so a platform named here and
  # not built there stops every release until one of the two is corrected.
  PLATFORMS=(
    "darwin-aarch64:Sync_macOS_aarch64.app.tar.gz"
  )

  SIG_DIR="$(mktemp -d)"
  trap 'rm -rf "$SIG_DIR"' EXIT
  MANIFEST_ARGS=()
  for entry in "${PLATFORMS[@]}"; do
    key="${entry%%:*}"
    name="${entry#*:}"
    if [ -n "$DIST" ]; then
      for f in "$DIST/$name" "$DIST/$name.sig"; do
        if [ ! -f "$f" ]; then
          echo "release: missing $f" >&2
          exit 1
        fi
      done
      sig_file="$DIST/$name.sig"
    else
      # Both the package and its signature have to be on the release already: a
      # manifest missing one platform is an update path broken for that platform
      # only, which is the kind of thing nobody notices for a month.
      if ! curl -fsIL "$base/$name" -o /dev/null; then
        echo "release: $base/$name is not downloadable — has the $TAG build finished?" >&2
        exit 1
      fi
      sig_file="$SIG_DIR/$key.sig"
      if ! curl -fsSL "$base/$name.sig" -o "$sig_file"; then
        echo "release: $base/$name.sig is missing — was the build run with TAURI_SIGNING_PRIVATE_KEY set?" >&2
        exit 1
      fi
    fi
    MANIFEST_ARGS+=("$key" "$base/$name" "$sig_file")
  done

  mkdir -p "$(dirname "$MANIFEST_OUT")"
  V="$VERSION" D="$(date -u +%Y-%m-%dT%H:%M:%SZ)" perl -MJSON::PP -e '
    my %platforms;
    while (@ARGV) {
      my ($key, $url, $sig_path) = splice(@ARGV, 0, 3);
      open my $fh, "<", $sig_path or die "read $sig_path: $!";
      local $/; my $sig = <$fh>;
      $sig =~ s/\s+\z//;
      $platforms{$key} = { url => $url, signature => $sig };
    }
    print JSON::PP->new->pretty->canonical->encode({
      version   => $ENV{V},
      pub_date  => $ENV{D},
      platforms => \%platforms,
    });
  ' "${MANIFEST_ARGS[@]}" > "$MANIFEST_OUT"

  echo "release: wrote $MANIFEST_OUT for $TAG"
  if [ -n "$OUT" ]; then
    echo "release: --out given — committing nothing."
    exit 0
  fi

  BRANCH="$(git rev-parse --abbrev-ref HEAD)"
  if [ "$BRANCH" != "main" ]; then
    echo "release: note — on branch '$BRANCH', not 'main'; installed copies poll main" >&2
  fi
  git add "$MANIFEST_OUT"
  git commit -m "release: publish the $TAG update manifest"
  if [ "$PUSH" -eq 1 ]; then
    git push origin "$BRANCH"
    echo "release: pushed — installed copies will take $TAG on their next check."
  else
    echo "release: committed, not pushed. To go live:"
    echo "    git push origin $BRANCH"
  fi
  exit 0
fi

# --- locate cargo ------------------------------------------------------------
CARGO="${CARGO:-}"
if [ -z "$CARGO" ]; then
  if command -v cargo >/dev/null 2>&1; then
    CARGO="cargo"
  elif [ -x "$HOME/.cargo/bin/cargo" ]; then
    CARGO="$HOME/.cargo/bin/cargo"
  else
    echo "release: cargo is not on PATH or in ~/.cargo/bin" >&2
    exit 1
  fi
fi

# --- pre-flight --------------------------------------------------------------
CURRENT="$(perl -ne 'if (/"version":\s*"([^"]+)"/) { print $1; exit }' "$TAURI_CONF")"
echo "release: $CURRENT -> $VERSION  (tag $TAG)"

if [ "$CURRENT" = "$VERSION" ]; then
  echo "release: the bundle is already at $VERSION — nothing to bump" >&2
  exit 1
fi

if git rev-parse -q --verify "refs/tags/${TAG}" >/dev/null; then
  echo "release: tag $TAG already exists" >&2
  exit 1
fi

if [ "$DRY_RUN" -eq 0 ] && ! git diff-index --quiet HEAD --; then
  echo "release: working tree has uncommitted changes — commit or stash first" >&2
  git status --short >&2
  exit 1
fi

BRANCH="$(git rev-parse --abbrev-ref HEAD)"
if [ "$BRANCH" != "main" ]; then
  echo "release: note — on branch '$BRANCH', not 'main'" >&2
fi

if [ "$DRY_RUN" -eq 1 ]; then
  echo "release: --dry-run, nothing written. Would set the version to $VERSION,"
  echo "         refresh Cargo.lock, commit 'release: $TAG' and tag $TAG."
  exit 0
fi

# --- the source of truth, and its two mirrors --------------------------------
# The first "version" key of each file. In tauri.conf.json that is the bundle's,
# above everything a plugin contributes; in Cargo.toml the anchored pattern
# matches the package's own bare line and not the inline `{ version = "..." }`
# tables of the dependencies below it.
V="$VERSION" perl -i -pe 'if (!$d && /"version":/) { s/"version": "[^"]*"/"version": "$ENV{V}"/; $d=1 }' "$TAURI_CONF"
V="$VERSION" perl -i -pe 'if (!$d && /^version = "/) { s/"[^"]*"/"$ENV{V}"/; $d=1 }' "$CARGO_TOML"
V="$VERSION" perl -i -pe 'if (!$d && /"version":/) { s/"version": "[^"]*"/"version": "$ENV{V}"/; $d=1 }' "$PKG_JSON"

# --- refresh the lockfile so the commit is self-consistent -------------------
( cd "$REPO_ROOT/src-tauri" && { "$CARGO" update --workspace --offline 2>/dev/null || "$CARGO" update --workspace; } )

# --- commit and tag ----------------------------------------------------------
git add "$TAURI_CONF" "$CARGO_TOML" "$PKG_JSON" "$REPO_ROOT/src-tauri/Cargo.lock"
git commit -m "release: $TAG"
git tag -a "$TAG" -m "$TAG"

echo
echo "release: committed and tagged $TAG."
if [ "$PUSH" -eq 1 ]; then
  echo "release: pushing $BRANCH and $TAG ..."
  git push origin "$BRANCH" "$TAG"
  echo "release: pushed — the release workflow will build from $TAG."
else
  echo "release: not pushed. To start the release build:"
  echo "    git push origin $BRANCH $TAG"
fi

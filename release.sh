#!/usr/bin/env bash
# Local release driver. Enforces ordering:
#   1. push tag                 → triggers CI to build + GH release + bump scoop/brew
#   2. wait for CI green        → fail-fast if build/release fails
#   3. verify release artifacts → no point publishing if binaries missing
#   4. cargo publish            → crates.io
#   5. publish both AUR packages
#
# Idempotent where it can be: re-running after a partial failure skips done steps.
# Refuses to run on a dirty working tree or with a missing version match.
#
# Usage:
#   ./release.sh             # uses version from Cargo.toml
#   ./release.sh 0.7.8       # asserts Cargo.toml matches argument

set -euo pipefail

cd "$(dirname "$0")"

cargo_version=$(grep -m1 '^version' Cargo.toml | sed -E 's|.*"(.+)".*|\1|')
VERSION="${1:-$cargo_version}"
TAG="v$VERSION"

[ "$VERSION" = "$cargo_version" ] || {
  echo "FATAL: requested $VERSION but Cargo.toml says $cargo_version" >&2
  exit 1
}

# ---- preflight ----
command -v gh    >/dev/null || { echo "FATAL: gh not on PATH" >&2; exit 1; }
command -v cargo >/dev/null || { echo "FATAL: cargo not on PATH" >&2; exit 1; }
command -v jq    >/dev/null || { echo "FATAL: jq not on PATH" >&2; exit 1; }
gh auth status >/dev/null 2>&1 || { echo "FATAL: gh not authenticated (gh auth login)" >&2; exit 1; }

if [ -n "$(git status --porcelain)" ]; then
  echo "FATAL: working tree is dirty. Commit or stash before releasing." >&2
  git status --short >&2
  exit 1
fi

current_branch=$(git branch --show-current)
[ "$current_branch" = "master" ] || {
  echo "WARN: not on master (on $current_branch). Continue? [y/N]"
  read -r ans
  [ "$ans" = "y" ] || exit 1
}

echo "Releasing $TAG..."

# ---- 1. tag (idempotent) ----
if git rev-parse "$TAG" >/dev/null 2>&1; then
  echo "  tag $TAG already exists locally, skipping create"
else
  git tag "$TAG"
fi

if git ls-remote --tags origin "$TAG" | grep -q "$TAG"; then
  echo "  tag $TAG already on remote, skipping push"
else
  git push origin "$TAG"
fi

# ---- 2. wait for CI ----
echo "Waiting for release workflow on $TAG..."
# Give GitHub a moment to register the run after the push.
sleep 5

# Find the workflow run for this tag. Try a few times since registration is async.
RUN_ID=""
for i in 1 2 3 4 5; do
  RUN_ID=$(gh run list --workflow=release.yml --limit=20 \
           --json databaseId,headBranch,event,status \
           -q ".[] | select(.event == \"push\" and (.headBranch == \"$TAG\")) | .databaseId" \
           | head -1)
  [ -n "$RUN_ID" ] && break
  sleep 5
done

if [ -z "$RUN_ID" ]; then
  echo "FATAL: no workflow run found for tag $TAG. Check 'gh run list' manually." >&2
  exit 1
fi

echo "  watching run $RUN_ID (gh run view $RUN_ID --web)"
gh run watch "$RUN_ID" --exit-status

# ---- 3. verify release artifacts ----
asset_count=$(gh release view "$TAG" --json assets -q '.assets | length' 2>/dev/null || echo 0)
if [ "$asset_count" -lt 4 ]; then
  echo "FATAL: release $TAG has only $asset_count assets; expected ≥4 (4 targets × .tar.gz/.zip)." >&2
  exit 1
fi
echo "  release $TAG has $asset_count assets ✓"

# ---- 4. cargo publish (skip if already published) ----
if cargo search rustgraph --limit 1 2>/dev/null | grep -qE "^rustgraph = \"$VERSION\""; then
  echo "  rustgraph $VERSION already on crates.io, skipping publish"
else
  echo "Publishing to crates.io..."
  cargo publish --locked
fi

# ---- 5. AUR push (rustgraph + rustgraph-bin) ----
command -v makepkg    >/dev/null || { echo "FATAL: makepkg not on PATH (pacman -S base-devel)" >&2; exit 1; }
command -v updpkgsums >/dev/null || { echo "FATAL: updpkgsums not on PATH (pacman -S pacman-contrib)" >&2; exit 1; }

push_aur() {
  local pkgname="$1"
  local src_dir="aur/$pkgname"

  [ -f "$src_dir/PKGBUILD" ] || { echo "FATAL: $src_dir/PKGBUILD missing" >&2; return 1; }

  local workdir
  workdir=$(mktemp -d -t "aur-${pkgname}-XXXXXX")

  echo "  $pkgname: clone aur:$pkgname"
  if ! git clone --quiet "ssh://aur@aur.archlinux.org/${pkgname}.git" "$workdir"; then
    rm -rf "$workdir"
    echo "FATAL: AUR clone failed for $pkgname (check ssh config + key for aur.archlinux.org)" >&2
    return 1
  fi

  cp "$src_dir/PKGBUILD" "$workdir/PKGBUILD"
  [ -f "$src_dir/rustgraph.install" ] && cp "$src_dir/rustgraph.install" "$workdir/rustgraph.install"

  (
    set -e
    cd "$workdir"
    updpkgsums                         # downloads sources, computes sha256
    makepkg --printsrcinfo > .SRCINFO  # AUR requires .SRCINFO alongside PKGBUILD

    git add PKGBUILD .SRCINFO
    [ -f rustgraph.install ] && git add rustgraph.install

    if git diff --staged --quiet; then
      echo "  $pkgname: no staged changes, skipping push"
    else
      git commit -m "release $VERSION"
      git push
      echo "  $pkgname: pushed ✓"
    fi
  )
  local rc=$?
  rm -rf "$workdir"
  return $rc
}

echo "Pushing to AUR..."
push_aur rustgraph
push_aur rustgraph-bin

echo
echo "============================================================"
echo "  release $TAG fully shipped:"
echo "    GH release v$VERSION + binaries"
echo "    crates.io: rustgraph $VERSION"
echo "    AUR: rustgraph + rustgraph-bin"
echo "    scoop + brew manifests bumped (via CI)"
echo "============================================================"

#!/usr/bin/env bash
set -euo pipefail

version="${1:-}"
tag="v$version"

if [[ -z "$version" ]]; then
  echo "usage: scripts/release.sh <version>" >&2
  exit 2
fi
if [[ "$(git branch --show-current)" != "main" ]]; then
  echo "release must run from main" >&2
  exit 1
fi
if [[ -n "$(git status --porcelain)" ]]; then
  echo "release requires a clean worktree" >&2
  exit 1
fi

git fetch origin main --tags
if [[ "$(git rev-parse HEAD)" != "$(git rev-parse origin/main)" ]]; then
  echo "local main must match origin/main" >&2
  exit 1
fi
if git rev-parse --verify --quiet "refs/tags/$tag" >/dev/null; then
  echo "tag already exists: $tag" >&2
  exit 1
fi

scripts/check-release-version.sh "$tag"
cargo fmt --all --check
cargo clippy --locked --all-targets --all-features -- -D warnings
cargo test --locked --all-targets --all-features

git tag "$tag"
git push origin "$tag"
echo "pushed $tag; the Release workflow will publish the GitHub Release after its gates pass"

#!/usr/bin/env bash
set -euo pipefail

version="${1:-}"
if [[ -z "$version" ]]; then
  echo "usage: scripts/update-homebrew.sh <version>" >&2
  exit 2
fi

tag="v$version"
repo="sorafujitani/rt"
formula="sorafujitani/tap/rt"
tap_repo_name="sorafujitani/homebrew-tap"
url="https://github.com/$repo/archive/refs/tags/$tag.tar.gz"

gh release view "$tag" --repo "$repo" >/dev/null
archive=$(mktemp)
trap 'rm -f "$archive"' EXIT
curl --fail --location --silent --show-error "$url" --output "$archive"
sha256=$(shasum -a 256 "$archive" | awk '{print $1}')

tap_repo=$(brew --repository sorafujitani/tap)
branch="rt-v$version"
if [[ -n "$(git -C "$tap_repo" status --porcelain)" ]]; then
  echo "Homebrew tap worktree must be clean: $tap_repo" >&2
  exit 1
fi

git -C "$tap_repo" fetch origin main
git -C "$tap_repo" switch main
git -C "$tap_repo" pull --ff-only origin main
git -C "$tap_repo" switch -c "$branch"

brew bump-formula-pr \
  --url "$url" \
  --sha256 "$sha256" \
  --write-only \
  --no-audit \
  "$formula"

brew audit --strict "$formula"
git -C "$tap_repo" add Formula/rt.rb
git -C "$tap_repo" commit -m "rt $version"
git -C "$tap_repo" push -u origin "$branch"
gh pr create \
  --repo "$tap_repo_name" \
  --base main \
  --head "$branch" \
  --title "Update rt to $version" \
  --body "Update rt to $tag and verify the release archive checksum."

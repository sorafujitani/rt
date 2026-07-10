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
url="https://github.com/$repo/archive/refs/tags/$tag.tar.gz"

gh release view "$tag" --repo "$repo" >/dev/null
archive=$(mktemp)
trap 'rm -f "$archive"' EXIT
curl --fail --location --silent --show-error "$url" --output "$archive"
sha256=$(shasum -a 256 "$archive" | awk '{print $1}')

brew bump-formula-pr \
  --version "$version" \
  --url "$url" \
  --sha256 "$sha256" \
  --strict \
  --no-browse \
  "$formula"

#!/usr/bin/env bash
set -euo pipefail

metadata=$(cargo metadata --locked --no-deps --format-version 1)
version=$(python3 -c 'import json,sys; data=json.load(sys.stdin); print(next(p["version"] for p in data["packages"] if p["name"] == "rt"))' <<<"$metadata")
tag="${1:-v$version}"
if [[ ! "$tag" =~ ^v[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
  echo "expected a release tag like v0.0.5, got: $tag" >&2
  exit 1
fi

if [[ "$tag" != "v$version" ]]; then
  echo "release tag $tag does not match Cargo version $version" >&2
  exit 1
fi

echo "release identity verified: $tag"

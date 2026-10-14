#!/usr/bin/env bash
# Git merge driver for Cargo.lock, installed by `just hooks`.
#
# A lockfile conflict is never resolved by hand: Cargo rebuilds it from the
# merged manifests. When the three-way merge is clean it is kept. Otherwise
# this keeps our side, records that the lockfile is stale, and the
# pre-commit or pre-merge-commit hook runs Cargo before the merge is
# committed, once every Cargo.toml has its merged contents.
set -euo pipefail

base=$1 ours=$2 theirs=$3
merged=$(mktemp)
trap 'rm -f "$merged"' EXIT

cp "$ours" "$merged"
if git merge-file --quiet "$merged" "$base" "$theirs"; then
  cp "$merged" "$ours"
  exit 0
fi

touch "$(git rev-parse --git-path herdr-cargo-lock-stale)"
echo "Cargo.lock: kept our side; it is rebuilt with Cargo before the commit." >&2

#!/usr/bin/env bash
# Rebuild Cargo.lock after the merge driver kept one side of a conflict.
# Run by the pre-commit and pre-merge-commit hooks; a no-op otherwise.
# Git does not reread the index after pre-merge-commit, so that hook passes
# --stop: the rebuilt lockfile is staged and the merge waits for `git commit`.
set -euo pipefail

marker=$(git rev-parse --git-path herdr-cargo-lock-stale)
[[ -e "$marker" ]] || exit 0

# Only the merge that left the marker may rebuild the lockfile. An aborted
# merge, or a rebase or cherry-pick that hit the driver, leaves nothing to do
# here: its marker is discarded rather than applied to an unrelated commit.
stop=${1-}
merging=false
if [[ "$stop" == --stop ]] || git rev-parse --verify --quiet MERGE_HEAD >/dev/null; then
  merging=true
fi
if [[ "$merging" != true || "$(cat "$marker")" != "$(git rev-parse --verify --quiet HEAD || true)" ]]; then
  rm -f "$marker"
  exit 0
fi

# Cargo resolves from the working tree, but the commit records the index. A
# manifest or lockfile edit left unstaged would end up in the staged lockfile.
if ! git diff --quiet -- Cargo.lock ':(glob)**/Cargo.toml'; then
  echo "Cargo.lock: stage or discard the unstaged Cargo.toml and Cargo.lock changes, then commit again; the lockfile is rebuilt from the staged manifests." >&2
  exit 1
fi

# Resolving adds only what the merged manifests need, at the versions our side
# already locks; it reaches the network only when a new package is missing.
if ! cargo metadata --format-version 1 --offline >/dev/null 2>&1 &&
  ! cargo metadata --format-version 1 >/dev/null; then
  echo "Cargo.lock: could not rebuild it. Run \`cargo metadata\`, stage Cargo.lock, and commit again." >&2
  exit 1
fi
git add Cargo.lock
rm -f "$marker"
if [[ "$stop" == --stop ]]; then
  echo "Cargo.lock: rebuilt from the merged manifests and staged. Run \`git commit --no-edit\` to finish the merge." >&2
  exit 1
fi
echo "Cargo.lock: rebuilt from the merged manifests and staged." >&2

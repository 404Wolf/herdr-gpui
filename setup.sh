#!/usr/bin/env bash
set -euo pipefail

mise trust

# Set a worktree-specific state directory so multiple instances can run
# simultaneously from different worktrees. The control socket lives here,
# and only one app per state directory can run.
worktree_name=$(basename "$PWD")
export XDG_STATE_HOME="${XDG_STATE_HOME:-$HOME/.local/state}/herdr-gpui-$worktree_name"
mkdir -p "$XDG_STATE_HOME"

echo "State directory: $XDG_STATE_HOME"
echo "Run 'source setup.sh' or add to your shell to enable multi-instance support."

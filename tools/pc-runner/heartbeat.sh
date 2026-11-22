#!/usr/bin/env bash
# Sets BPE_PC_HEARTBEAT to the current Unix time while GitHub sees at least one bpe-pc runner online, so CI's "What
# changed" job knows the PC can take the checks. Runs as the beat user, whose GitHub login jobs can't read.
set -euo pipefail
repo=pixelpitch161/BallPhysicsEngine
online=$(gh api "repos/$repo/actions/runners" \
  --jq '[.runners[] | select(.status == "online" and any(.labels[]; .name == "bpe-pc"))] | length')
[ "$online" -gt 0 ] || exit 0
gh variable set BPE_PC_HEARTBEAT --repo "$repo" --body "$(date +%s)"

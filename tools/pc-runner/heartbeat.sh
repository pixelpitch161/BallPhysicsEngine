#!/usr/bin/env bash
# Sets BPE_PC_HEARTBEAT to the current Unix time while at least one runner is running, so CI's "What changed" job
# knows the PC can take the checks. Runs as the runner user with its own gh login.
set -euo pipefail
systemctl list-units --state=active --plain --no-legend 'bpe-runner@*' | grep -q . || exit 0
gh variable set BPE_PC_HEARTBEAT --repo pixelpitch161/BallPhysicsEngine --body "$(date +%s)"

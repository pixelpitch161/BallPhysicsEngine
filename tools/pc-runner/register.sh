#!/usr/bin/env bash
# Registers the runners with GitHub and starts everything (plan step 3). Runs as root inside bpe-ci, after the runner
# user's `gh auth login`. Safe to run again (--replace). The registration token goes straight from gh to config.sh.
set -euo pipefail
repo=pixelpitch161/BallPhysicsEngine
runners=3
for n in $(seq 1 $runners); do
  d=/home/runner/actions-runner-$n
  if [ ! -f "$d/.runner" ]; then
    su - runner -c "cd $d && ./config.sh --unattended --url https://github.com/$repo --name bpe-pc-$n --labels bpe-pc \
      --work _work --replace --token \"\$(gh api -X POST repos/$repo/actions/runners/registration-token --jq .token)\""
  fi
done
systemctl daemon-reload
systemctl enable --now $(for n in $(seq 1 $runners); do echo "bpe-runner@$n"; done) bpe-heartbeat.timer \
  bpe-target-sweep.timer
echo "register done"

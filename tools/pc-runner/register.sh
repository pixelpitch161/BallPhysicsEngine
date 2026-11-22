#!/usr/bin/env bash
# Registers the runners with GitHub and starts everything (plan step 3). Runs as root inside bpe-ci, after the beat
# user's `gh auth login`. Safe to run again: a runner GitHub doesn't list (it deletes runners offline for 14 days) is
# registered afresh. The one-hour registration token goes straight from gh to config.sh.
set -euo pipefail
repo=pixelpitch161/BallPhysicsEngine
runners=3
known=$(su - beat -c "gh api repos/$repo/actions/runners --jq '.runners[].name'")
for n in $(seq 1 $runners); do
  d=/home/runner/actions-runner-$n
  if ! grep -qx "bpe-pc-$n" <<<"$known"; then
    systemctl stop "bpe-runner@$n" 2>/dev/null || true
    rm -f "$d/.runner" "$d/.credentials" "$d/.credentials_rsaparams"
    token=$(su - beat -c "gh api -X POST repos/$repo/actions/runners/registration-token --jq .token")
    su - runner -c "cd $d && ./config.sh --unattended --url https://github.com/$repo --name bpe-pc-$n --labels bpe-pc \
      --work _work --replace --token $token"
  fi
done
systemctl daemon-reload
systemctl enable --now $(for n in $(seq 1 $runners); do echo "bpe-runner@$n"; done) bpe-heartbeat.timer
echo "register done"

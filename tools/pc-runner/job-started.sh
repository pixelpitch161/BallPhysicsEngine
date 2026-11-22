#!/usr/bin/env bash
# Runs before every job on the PC (ACTIONS_RUNNER_HOOK_JOB_STARTED). Keeps the kept build files
# (/home/runner/target/<job>, set by ci.yml) from filling the disk: past 150 GB they are all deleted, but only when no
# other runner has a job going, so nothing is deleted under a running build. The next jobs rebuild from scratch.
set -euo pipefail
dir=/home/runner/target
[ -d "$dir" ] || exit 0
used=$(du -s --block-size=1G "$dir" | cut -f1)
[ "$used" -gt 150 ] || exit 0
if [ "$(pgrep -c -f 'bin/Runner.Worker')" -le 1 ]; then
  find "$dir" -mindepth 1 -maxdepth 1 -exec rm -rf {} +
  echo "Emptied the kept build files ($used GB); this job rebuilds from scratch."
fi

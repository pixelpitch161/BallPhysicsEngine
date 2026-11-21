#!/usr/bin/env bash
# Empties /home/runner/target when it passes 150 GB.
set -euo pipefail
used=$(du -s --block-size=1G /home/runner/target | cut -f1)
if [ "$used" -gt 150 ]; then
  find /home/runner/target -mindepth 1 -maxdepth 1 -exec rm -rf {} +
  echo "swept $used GB"
fi

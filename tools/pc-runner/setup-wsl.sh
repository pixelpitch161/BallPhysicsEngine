#!/usr/bin/env bash
# Linux half of the PC test runner setup (docs/superpowers/plans/2026-11-21-pc-test-runner.md, step 2). Runs as
# root inside the bpe-ci WSL distro, with this folder's files (and the repo's rust-toolchain.toml) in /root/pc-runner.
# Safe to run again.
set -euo pipefail
export CARGO_NET_RETRY=10
here=/root/pc-runner
runners=4
nextest=0.9.152 # the version ci.yml pins; move both together

export DEBIAN_FRONTEND=noninteractive
# This PC's Wi-Fi sometimes damages downloads (hash mismatches, TLS errors), so retry instead of stopping.
echo 'Acquire::Retries "10";' > /etc/apt/apt.conf.d/80-retries
for i in 1 2 3 4 5; do apt-get update -o APT::Update::Error-Mode=any && break; sleep 5; done
# Up to date like GitHub's machines (their image takes Ubuntu's updates weekly; libc's maths and Mesa must match).
apt-get upgrade -y
# GitHub's own gh package: Ubuntu's 2.45 gives up on the device login ("slow_down").
if [ ! -f /etc/apt/sources.list.d/github-cli.list ]; then
  install -d /etc/apt/keyrings
  curl -fsSL --retry 10 --retry-all-errors -o /etc/apt/keyrings/githubcli-archive-keyring.gpg \
    https://cli.github.com/packages/githubcli-archive-keyring.gpg
  echo "deb [arch=amd64 signed-by=/etc/apt/keyrings/githubcli-archive-keyring.gpg] https://cli.github.com/packages stable main" \
    > /etc/apt/sources.list.d/github-cli.list
  for i in 1 2 3 4 5; do apt-get update -o APT::Update::Error-Mode=any && break; sleep 5; done
fi
# The packages the hosted jobs install (ci.yml), plus what the runner and the heartbeat need.
apt-get install -y --no-install-recommends \
  libasound2-dev pkg-config libudev-dev mesa-vulkan-drivers ffmpeg libxkbcommon-x11-0 libxkbcommon0 \
  gcc-mingw-w64-x86-64 build-essential python3 git curl ca-certificates jq gh vulkan-tools libicu74 zstd

# `runner` runs the jobs. `beat` holds the GitHub login (heartbeat, registration); jobs can't read its home.
id runner >/dev/null 2>&1 || useradd --create-home --shell /bin/bash runner
id beat >/dev/null 2>&1 || useradd --create-home --shell /bin/bash beat
chmod 700 /home/beat

# rustup without a default toolchain: rust-toolchain.toml picks the version, as on the hosted machines.
if [ ! -x /home/runner/.cargo/bin/rustup ]; then
  su - runner -c 'curl --proto =https --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal --default-toolchain none'
fi
# The pinned toolchain up front, so the four runners don't race to install it (CI also takes a lock).
toolchain=$(tr -d '\r' < "$here/rust-toolchain.toml" | sed -n 's/^channel = "\(.*\)"$/\1/p')
su - runner -c "RUSTUP_MAX_RETRIES=10 ~/.cargo/bin/rustup toolchain install $toolchain --profile minimal \
  --component rustfmt --component clippy --target x86_64-pc-windows-gnu"
# nextest once here (ci.yml installs it only on GitHub's machines: four jobs writing ~/.cargo/bin at once collide).
if ! su - runner -c "~/.cargo/bin/cargo-nextest nextest --version 2>/dev/null" | grep -q "nextest $nextest"; then
  su - runner -c "curl -fsSL --retry 10 --retry-all-errors https://get.nexte.st/$nextest/linux | tar -xzf - -C ~/.cargo/bin"
fi

# The GitHub Actions runner, one copy per runner.
release=$(curl -fsSL --retry 10 --retry-all-errors https://api.github.com/repos/actions/runner/releases/latest)
ver=$(jq -r .tag_name <<<"$release" | sed 's/^v//')
tar=/home/runner/actions-runner-linux-x64-$ver.tar.gz
if [ ! -f "$tar" ]; then
  curl -fsSL --retry 10 --retry-all-errors -C - -o "$tar.part" \
    "https://github.com/actions/runner/releases/download/v$ver/actions-runner-linux-x64-$ver.tar.gz"
  # The release notes carry each file's SHA-256 between "BEGIN SHA linux-x64" markers.
  want=$(jq -r .body <<<"$release" | grep -o 'BEGIN SHA linux-x64 -->[0-9a-f]\{64\}' | grep -o '[0-9a-f]\{64\}' || true)
  if [ -z "$want" ]; then echo "no linux-x64 checksum in the runner release notes; check by hand" >&2; exit 1; fi
  got=$(sha256sum "$tar.part" | cut -d' ' -f1)
  if [ "$want" != "$got" ]; then echo "runner download checksum mismatch ($want vs $got)" >&2; rm -f "$tar.part"; exit 1; fi
  mv "$tar.part" "$tar"
  chown runner:runner "$tar"
fi
install -m 0755 "$here/job-started.sh" /usr/local/bin/bpe-job-started.sh
for n in $(seq 1 $runners); do
  d=/home/runner/actions-runner-$n
  if [ ! -f "$d/run.sh" ]; then
    su - runner -c "mkdir -p $d && tar -xzf $tar -C $d"
  fi
  # Read by the service unit: Mesa's software driver only (the same as GitHub's machines; two threads per test, as
  # the pc nextest profile runs 12 at a time), retries for this PC's
  # network, and the hook that sweeps old build files before a job starts.
  cat > "$d/.env" <<EOF
VK_DRIVER_FILES=/usr/share/vulkan/icd.d/lvp_icd.json
VK_ICD_FILENAMES=/usr/share/vulkan/icd.d/lvp_icd.json
GALLIUM_DRIVER=llvmpipe
LP_NUM_THREADS=2
LANG=C.UTF-8
CARGO_NET_RETRY=10
RUSTUP_MAX_RETRIES=10
ACTIONS_RUNNER_HOOK_JOB_STARTED=/usr/local/bin/bpe-job-started.sh
EOF
  chown runner:runner "$d/.env"
done
install -d -o runner -g runner /home/runner/target

install -m 0755 "$here/heartbeat.sh" /usr/local/bin/bpe-heartbeat
install -m 0755 "$here/register.sh" /usr/local/sbin/bpe-register
for f in bpe-runner@.service bpe-heartbeat.service bpe-heartbeat.timer; do
  install -m 0644 "$here/$f" /etc/systemd/system/$f
done
# The weekly sweep of earlier versions deleted build files under running jobs; the job-start hook replaces it.
systemctl disable --now bpe-target-sweep.timer 2>/dev/null || true
rm -f /etc/systemd/system/bpe-target-sweep.{service,timer} /usr/local/bin/bpe-target-sweep
systemctl daemon-reload || true

# systemd on; no Windows drives and no Windows programs, so a job can't reach the owner's files.
cat > /etc/wsl.conf <<'EOF'
[boot]
systemd=true

[automount]
enabled=false
mountFsTab=false

[interop]
enabled=false
appendWindowsPath=false

[user]
default=runner
EOF
echo "setup-wsl done"

#!/usr/bin/env bash
# Linux half of the PC test runner setup (docs/superpowers/plans/2026-11-21-pc-test-runner.md, step 2). Runs as
# root inside the bpe-ci WSL distro, with this folder's files in /root/pc-runner. Safe to run again.
set -euo pipefail
here=/root/pc-runner
runners=3

export DEBIAN_FRONTEND=noninteractive
apt-get update
# The packages the hosted jobs install (ci.yml), plus what the runner and the heartbeat need.
apt-get install -y --no-install-recommends \
  libasound2-dev pkg-config libudev-dev mesa-vulkan-drivers ffmpeg libxkbcommon-x11-0 libxkbcommon0 \
  gcc-mingw-w64-x86-64 build-essential python3 git curl ca-certificates jq gh vulkan-tools libicu74 zstd

id runner >/dev/null 2>&1 || useradd --create-home --shell /bin/bash runner

# rustup without a default toolchain: rust-toolchain.toml picks the version, as on the hosted machines.
if [ ! -x /home/runner/.cargo/bin/rustup ]; then
  su - runner -c 'curl --proto =https --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal --default-toolchain none'
fi

# The GitHub Actions runner, one copy per runner.
release=$(curl -fsSL https://api.github.com/repos/actions/runner/releases/latest)
ver=$(jq -r .tag_name <<<"$release" | sed 's/^v//')
tar=/home/runner/actions-runner-linux-x64-$ver.tar.gz
if [ ! -f "$tar" ]; then
  curl -fsSL -o "$tar.part" "https://github.com/actions/runner/releases/download/v$ver/actions-runner-linux-x64-$ver.tar.gz"
  # The release notes carry each file's SHA-256 between "BEGIN SHA linux-x64" markers.
  want=$(jq -r .body <<<"$release" | grep -o 'BEGIN SHA linux-x64 -->[0-9a-f]\{64\}' | grep -o '[0-9a-f]\{64\}')
  got=$(sha256sum "$tar.part" | cut -d' ' -f1)
  if [ "$want" != "$got" ]; then echo "runner download checksum mismatch ($want vs $got)" >&2; rm -f "$tar.part"; exit 1; fi
  mv "$tar.part" "$tar"
  chown runner:runner "$tar"
fi
for n in $(seq 1 $runners); do
  d=/home/runner/actions-runner-$n
  if [ ! -f "$d/run.sh" ]; then
    su - runner -c "mkdir -p $d && tar -xzf $tar -C $d"
  fi
  # Read by the runner at start: build files outside the checkout (checkout's clean would delete them) and Mesa's
  # software driver only, the same as GitHub's machines (WSL also offers the real GPU through dzn).
  cat > "$d/.env" <<EOF
CARGO_TARGET_DIR=/home/runner/target/bpe-pc-$n
VK_DRIVER_FILES=/usr/share/vulkan/icd.d/lvp_icd.x86_64.json
VK_ICD_FILENAMES=/usr/share/vulkan/icd.d/lvp_icd.x86_64.json
GALLIUM_DRIVER=llvmpipe
LANG=C.UTF-8
EOF
  echo "PATH=/home/runner/.cargo/bin:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin" > "$d/.path"
  chown runner:runner "$d/.env" "$d/.path"
done
install -d -o runner -g runner /home/runner/target

install -m 0755 "$here/heartbeat.sh" /usr/local/bin/bpe-heartbeat
install -m 0755 "$here/target-sweep.sh" /usr/local/bin/bpe-target-sweep
for f in bpe-runner@.service bpe-heartbeat.service bpe-heartbeat.timer bpe-target-sweep.service bpe-target-sweep.timer; do
  install -m 0644 "$here/$f" /etc/systemd/system/$f
done

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

#!/usr/bin/env bash
# Build locally and ship the binary. Building here rather than on the instance
# is deliberate: cargo peaks around 1.1 GB and a t3.micro has 1 GB, so a build
# there dies partway through.
set -euo pipefail

HOST="${DEPLOY_HOST:-ubuntu@52.74.52.188}"
KEY="${DEPLOY_KEY:-$HOME/.ssh/cpgeeks-backend-key.pem}"

echo "==> building release binary"
cargo build --release --locked

echo "==> checking it will run on the target"
# glibc is the one that bites: the binary records the newest symbol version it
# needs, and the instance has to be at least that new
need=$(objdump -T target/release/backend | grep -oE 'GLIBC_[0-9.]+' | sort -V | tail -1)
have=$(ssh -i "$KEY" "$HOST" 'ldd --version | head -1 | grep -oE "[0-9]+\.[0-9]+$"')
echo "    binary needs ${need}, instance has glibc ${have}"

echo "==> uploading"
scp -i "$KEY" target/release/backend "$HOST:/tmp/backend.new"

echo "==> installing and restarting"
ssh -i "$KEY" "$HOST" '
  sudo install -o cpgeeks -g cpgeeks -m 0755 /tmp/backend.new /opt/cpgeeks/backend &&
  rm -f /tmp/backend.new &&
  sudo systemctl restart cpgeeks-backend &&
  sleep 3 &&
  systemctl is-active cpgeeks-backend
'

echo "==> health check"
curl -fsS https://api.sustcpgeeks.me/api/health && echo
echo "==> done"

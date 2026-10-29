#!/usr/bin/env bash
# CI runs on a self-hosted runner without sudo. Its build dependencies are
# preinstalled by the runner setup; fail fast with a clear list if any is missing.
set -euo pipefail
packages=(
  pkg-config libwayland-dev libxkbcommon-dev libxkbcommon-x11-dev
  libx11-xcb-dev libxcb1-dev libfontconfig-dev libfreetype-dev
  libvulkan-dev libasound2-dev libssl-dev libzstd-dev libsqlite3-dev libdbus-1-dev
)
missing=()
for package in "${packages[@]}"; do
  dpkg-query -W -f='${Status}' "$package" 2>/dev/null | grep -q 'install ok installed' \
    || missing+=("$package")
done
if ((${#missing[@]})); then
  echo "Runner is missing build dependencies: ${missing[*]}" >&2
  echo "Install them on the runner: sudo apt-get install -y ${missing[*]}" >&2
  exit 1
fi
echo "All ${#packages[@]} build dependencies are installed."

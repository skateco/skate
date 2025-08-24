#!/bin/bash
set -euo pipefail

# debug
if [[ -n "${DEBUG:-}" ]]; then
  set -x
fi

INSTALL_PATH=${INSTALL_PATH:-/usr/local/bin}
VERSION=${VERSION:-}

if [[ -z "$VERSION" ]]; then
  URL_SUFFIX="latest"
else
  URL_SUFFIX="tags/$VERSION"
fi

os=$(uname -o)
arch=$(uname -m)
vendor="unknown"

# make these more elegant later
if [[ "$os" == "GNU/Linux" ]]; then
  os="linux-gnu"
elif [[ "$os" == "Darwin" ]]; then
  os="darwin"
  vendor="apple"
else
  echo "Unsupported OS: $os"
  exit 1
fi

# make these more elegant later
if [[ "$arch" == *"x86_64"* ]]; then
  arch="x86_64"
elif [[ "$arch" == *"aarch64"* ]]; then
  arch="aarch64"
elif [[ "$arch" == *"arm64"* ]]; then
  arch="aarch64"
else
  echo "Unsupported architecture: $arch"
  exit 1
fi

declare -a EXTRA_ARGS=()
if [[ -n "${GITHUB_TOKEN:-}" ]]; then
  EXTRA_ARGS=("-H" "Authorization: Bearer $GITHUB_TOKEN")
fi

get_install_alternatives(){
  # shellcheck disable=SC2068
  output=$(curl ${EXTRA_ARGS[@]+"${EXTRA_ARGS[@]}"} -f --retry 5 --retry-max-time 30 --retry-all-errors --silent "https://api.github.com/repos/skateco/skate/releases/${URL_SUFFIX}")

  echo "$output" \
    | grep "browser_download_url.*tar.gz" \
    | cut -d : -f 2,3 \
    | tr -d \\\" \
    | tr -d "[:blank:]"
}

# Find for our triple

triple="$arch-$vendor-$os"
echo "Triple: $triple"

archive_name="skatelet-$triple.tar.gz"

install_url=$(get_install_alternatives|grep "$archive_name" | head -n 1)

if [[ -z "$install_url" ]]; then
  echo "No install URL found for $archive_name"
  exit 1
fi

echo "Installing from $install_url"

rm -rf /tmp/skatelet-install
mkdir -p /tmp/skatelet-install
curl -sSL -o "/tmp/skatelet-install/skatelet.tar.gz" "$install_url"

cd /tmp/skatelet-install
tar -xvf skatelet.tar.gz



sudo mv skatelet "${INSTALL_PATH}/skatelet"
sudo chmod +x "${INSTALL_PATH}/skatelet"

echo "Skatelet installed successfully in ${INSTALL_PATH}"


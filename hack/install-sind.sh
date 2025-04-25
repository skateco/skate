#!/bin/bash
set -euo pipefail

# debug
if [[ -n "${DEBUG:-}" ]]; then
  set -x
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


get_install_alternatives(){
  output=$(curl --retry 5 --retry-max-time 30 --silent https://api.github.com/repos/skateco/skate/releases/latest)

  if [[ -n "${DEBUG:-}" ]]; then
    echo "$output"
  fi

  echo "$output" \
    | grep "browser_download_url.*tar.gz" \
    | cut -d : -f 2,3 \
    | tr -d \\\" \
    | tr -d "[:blank:]"
}

# Find for our triple

triple="$arch-$vendor-$os"
echo "Triple: $triple"

archive_name="sind-$triple.tar.gz"


install_url=$(get_install_alternatives|grep "$archive_name" | head -n 1)

if [[ -z "$install_url" ]]; then
  echo "No install URL found for $archive_name"
  exit 1
fi

echo "Installing from $install_url"

rm -rf /tmp/sind-install
mkdir -p /tmp/sind-install
curl -sSL -o "/tmp/sind-install/sind.tar.gz" "$install_url"

cd /tmp/sind-install
tar -xvf sind.tar.gz



sudo mv sind /usr/local/bin/sind
sudo chmod +x /usr/local/bin/sind

echo "Sind installed successfully in /usr/local/bin"


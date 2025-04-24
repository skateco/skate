#!/bin/bash
set -euo pipefail

if [ "$(arch)" = "x86_64" ]; then
  binary=target/debug/skatelet
  if [ ! -f $binary ]; then
    set -x
    cargo build --bin skatelet --locked
    set +x
  fi
else
  target=x86_64-unknown-linux-gnu
  binary=target/$target/debug/skatelet
  if [ ! -f $binary ]; then
    set -x
    cross build --bin skatelet --locked --target "$target"
    set +x
  fi
fi
for f in $(seq 2); do
    docker cp "$binary" "sind-node-$f":/usr/local/bin/skatelet
done

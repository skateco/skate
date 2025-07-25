#!/bin/bash
set -euo pipefail

butane(){

              # shellcheck disable=SC2068
  docker run --rm --interactive         \
              --security-opt label=disable          \
              --volume "${PWD}:/pwd" --workdir /pwd \
              quay.io/coreos/butane:release $@
    }


BUTANE_CONFIG="./config.bu"

IGNITION_CONFIG="./config.ign"

butane --pretty --strict $BUTANE_CONFIG > $IGNITION_CONFIG

vfkit --cpus 2 --memory 2048 \
  --bootloader efi,variable-store=efi-variable-store,create \
  --device "virtio-blk,path=${IMAGE_PATH}" \
  --device virtio-net,nat \
  --ignition "${IGNITION_CONFIG}" \
  --device virtio-input,keyboard \
  --device virtio-input,pointing \
  --device virtio-gpu,width=800,height=600 \
  --gui

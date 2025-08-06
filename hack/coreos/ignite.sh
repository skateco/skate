#!/bin/bash
set -euo pipefail

butane(){

              # shellcheck disable=SC2068
  docker run --rm --interactive         \
              --security-opt label=disable          \
              --volume "${PWD}:/pwd:ro" --workdir /pwd \
              quay.io/coreos/butane:release $@
    }

password_hash=$(docker run -i -q --rm quay.io/coreos/mkpasswd -s --method=yescrypt <<< skate)

cat <<EOF > ./config.bu
variant: fcos
version: 1.6.0
passwd:
  users:
    - name: skate
      password_hash: "$password_hash"
      ssh_authorized_keys:
        - $SSH_PUBLIC_KEY
      groups:
        - sudo
storage:
  files:
    - path: /etc/hostname
      mode: 0644
      contents:
        inline: |
          skatebox
EOF


IGNITION_CONFIG="./config.ign"

butane --pretty --strict ./config.bu > $IGNITION_CONFIG

vfkit --cpus 2 --memory 2048 \
  --bootloader efi,variable-store=efi-variable-store,create \
  --device "virtio-blk,path=${IMAGE_PATH}" \
  --device virtio-net,nat \
  --ignition "${IGNITION_CONFIG}" \
  --device virtio-input,keyboard \
  --device virtio-input,pointing \
  --device virtio-gpu,width=800,height=600 \
  --gui

#!/bin/sh
set -x -e

if [ ! -z "$CORE_FILE" ]; then
    echo "$CORE_FILE" > /Corefile
fi

exec /coredns "$@"
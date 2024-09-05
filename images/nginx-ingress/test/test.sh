#!/bin/bash

set -xeuo pipefail


cat 443.json|cargo run --bin skatelet template -f ../service.conf.tmpl -
cat 80.json|cargo run --bin skatelet template -f ../service.conf.tmpl -

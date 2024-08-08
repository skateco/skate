#!/bin/bash

set -xeuo pipefail

cat 443.json|skatelet template -f ../service.conf.tmpl -
cat 80.json|skatelet template -f ../service.conf.tmpl -

.ONE_SHELL:
.PHONY: default

SHELL := /bin/bash

default: aarch64

.PHONY: aarch64
aarch64:
	CFLAGS="" TARGET_CC=aarch64-linux-musl-gcc CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER=aarch64-linux-musl-gcc cargo build --target aarch64-unknown-linux-musl
aarch64-release:
	CFLAGS="" TARGET_CC=aarch64-unknown-linux-gnu-gcc CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER=aarch64-unknown-linux-gnu-gcc cargo build --target aarch64-unknown-linux-gnu --release --locked
.PHONY: aarch64-cross
aarch64-cross:
	cross -v build  --target aarch64-unknown-linux-gnu --release --locked
.PHONY: amd64
amd64:
	TARGET_CC=x86_64-unknown-linux-gnu-gcc cargo build --target=x86_64-unknown-linux-gnu
.PHONY: amd64-cross
amd64-cross:
	cross -v build --target=x86_64-unknown-linux-gnu  --release --locked

.PHONY: lint
lint:
	cargo clippy
.PHONY: lint-fix
lint-fix:
	cargo clippy --fix --all --allow-dirty --allow-staged

.PHONY: run-e2e-tests
run-e2e-tests: SSH_PRIVATE_KEY=/tmp/skate-e2e-key
run-e2e-tests:
	set -euo pipefail
	[ -f ${SSH_PRIVATE_KEY} ] || ssh-keygen -b 2048 -t rsa -f ${SSH_PRIVATE_KEY} -q -N ""
	echo "SSH_PRIVATE_KEY=${SSH_PRIVATE_KEY}" > ./hack/.clusterplz.env
	# start vms
	./hack/clusterplz create || exit 0
	cargo run --bin skate -- delete cluster e2e-test --yes || exit 0
	cargo run --bin skate -- create cluster e2e-test
	cargo run --bin skate -- config use-context e2e-test
	./hack/clusterplz skatelet
	./hack/clusterplz skate
    # the ignored tests are the e2e tests. This is not optimal.
	SKATE_E2E=1 cargo test --test '*' -v -- --show-output --nocapture

.PHONY: run-e2e-tests-docker
run-e2e-tests-docker: SSH_PRIVATE_KEY=/tmp/skate-e2e-key
run-e2e-tests-docker: SSH_PUBLIC_KEY=/tmp/skate-e2e-key.pub
run-e2e-tests-docker: export PATH := $(shell pwd)/target/release:${PATH}
run-e2e-tests-docker: export SKATELET_PATH ?= $(shell pwd)/target/release/skatelet
run-e2e-tests-docker:
	set -xeuo pipefail
	cargo build --release --locked --bin skate
	which skatelet
	[ -f ${SSH_PRIVATE_KEY} ] || ssh-keygen -b 2048 -t rsa -f ${SSH_PRIVATE_KEY} -q -N ""
	# start vms
	cargo run --bin sind -- create --ssh-private-key ${SSH_PRIVATE_KEY} --ssh-public-key ${SSH_PUBLIC_KEY} --skatelet-binary-path ${SKATELET_PATH}
	cargo run --bin skate -- config use-context sind
	SKATE_E2E=1 cargo test --test '*' -v -- --show-output --nocapture

.PHONY: verify-images-build
verify-images-build:
	cd ./images/coredns && make build
	cd ./images/nginx-ingress && make build
	cd ./images/sind && make build

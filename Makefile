.ONE_SHELL:
.PHONY: default

SHELL := /bin/bash

default: aarch64

.PHONY: aarch64
aarch64:
	CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER=aarch64-unknown-linux-gnu-gcc cargo build --target aarch64-unknown-linux-gnu
aarch64-release:
	CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER=aarch64-unknown-linux-gnu-gcc cargo build --target aarch64-unknown-linux-gnu --release --locked
.PHONY: aarch64-cross
aarch64-cross:
	cross build  --target aarch64-unknown-linux-gnu --release --locked
.PHONY: amd64
amd64:
	TARGET_CC=x86_64-unknown-linux-gnu-gcc cargo build --target=x86_64-unknown-linux-gnu
.PHONY: amd64-cross
amd64-cross:
	cross build --target=x86_64-unknown-linux-gnu  --release --locked

.PHONY: lint
lint:
	cargo clippy
.PHONY: lint-fix
lint-fix:
	cargo clippy --fix --all --allow-dirty --allow-staged



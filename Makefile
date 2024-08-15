.PHONY: default
default: aarch64

.PHONY: aarch64
aarch64:
	TARGET_CC=aarch64-unknown-linux-gnu-gcc cargo build --target aarch64-unknown-linux-gnu
.PHONY: aarch64-cross
aarch64-cross:
	cross build  --target aarch64-unknown-linux-gnu
.PHONY: amd64
amd64:
	TARGET_CC=x86_64-unknown-linux-gnu-gcc cargo build --target=x86_64-unknown-linux-gnu
.PHONY: amd64-cross
amd64-cross:
	cross build --target=x86_64-unknown-linux-gnu



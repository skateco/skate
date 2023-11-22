.PHONY: default
default: armv7

.PHONY: armv6
armv6: # for ras pi 1 NOTE: sometimes have to run it a few times to get the whole thing built ¯\_(ツ)_/¯
	cross build  --target arm-unknown-linux-gnueabi
.PHONY: armv7
armv7: # for ras pi 2, also have to rerun sometimes ¯\_(ツ)_/¯
	cross build  --target armv7-unknown-linux-gnueabi
.PHONY: arm64
aarch64:
	cross build  --target aarch64-unknown-linux-gnu
.PHONY: amd64 # on mac
amd64:
	CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_LINKER=x86_64-unknown-linux-gnu-gcc cargo build --target=x86_64-unknown-linux-gnu


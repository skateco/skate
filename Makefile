.PHONY: default
default: aarch64

.PHONY: aarch64
aarch64:
	CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER=aarch64-unknown-linux-gnu-gcc cargo build --target aarch64-unknown-linux-gnu
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


.PHONY: run-integration-tests
run-integration-tests:
	# start vms
	./hack/clusterplz create || exit 0
	# This copies over a skatelet binary. Not good, should really find a better way to do this.
	./hack/clusterplz skatelet
    # the ignored tests are the integration tests. This is not optimal.
	cargo test -- --include-ignored


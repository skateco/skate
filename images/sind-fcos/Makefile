.PHONY: build-and-push
build-and-push: guard-TAG
build-and-push:
	docker buildx build --platform linux/arm64,linux/amd64  -t ghcr.io/skateco/sind:${TAG}-fedora-coreos -t ghcr.io/skateco/sind:fedora-coreos --push .
.PHONY: build
build: guard-TAG
build:
	docker buildx build --platform linux/arm64,linux/amd64  -t ghcr.io/skateco/sind:${TAG}-fedora-coreos -t ghcr.io/skateco/sind:fedora-coreos .

guard-%:
	@ if [ "${${*}}" = "" ]; then \
		echo "Environment variable $* not set"; \
		exit 1; \
	fi

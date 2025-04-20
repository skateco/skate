.PHONY: build-and-push
build-and-push:
	docker buildx build --platform linux/arm64,linux/amd64  -t ghcr.io/skateco/sind:latest --push .
.PHONY: build
build:
	docker buildx build --platform linux/arm64,linux/amd64  -t ghcr.io/skateco/sind:latest .

.PHONY: build-and-push
build-and-push: guard-TAG
build-and-push:
	docker buildx build -f ContainerFile --platform linux/arm64,linux/amd64  -t ghcr.io/skateco/fcos-skate:${TAG} --push .
.PHONY: build
build: guard-TAG
build:
	docker buildx build -f ContainerFile --platform linux/arm64,linux/amd64  -t ghcr.io/skateco/fcos-skate:${TAG} .

guard-%:
	@ if [ "${${*}}" = "" ]; then \
		echo "Environment variable $* not set"; \
		exit 1; \
	fi

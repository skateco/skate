.PHONY: build-and-push
build-and-push: guard-TAG
build-and-push: build
	docker buildx build --platform linux/arm64,linux/amd64  -t ghcr.io/skateco/coredns:${TAG} --push .
.PHONY: build
build: guard-TAG
build: binaries
	docker buildx build --platform linux/arm64,linux/amd64  -t ghcr.io/skateco/coredns:${TAG} .

.PHONY: binaries
binaries:
#	GOOS=linux GOARCH=arm GOARM=6 CGO_ENABLED=0 go build -ldflags="-w -s" -o build/armv6/coredns .
#	GOOS=linux GOARCH=arm GOARM=7 CGO_ENABLED=0 go build -ldflags="-w -s" -o build/armv7/coredns .
	GOOS=linux GOARCH=arm64  CGO_ENABLED=0 go build -ldflags="-w -s" -o build/arm64/coredns .
	GOOS=linux GOARCH=amd64  CGO_ENABLED=0 go build -ldflags="-w -s" -o build/amd64/coredns .

guard-%:
	@ if [ "${${*}}" = "" ]; then \
		echo "Environment variable $* not set"; \
		exit 1; \
	fi
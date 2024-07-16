.PHONY: build-and-push
build-and-push:
#	GOOS=linux GOARCH=arm GOARM=6 CGO_ENABLED=0 go build -ldflags="-w -s" -o build/armv6/coredns .
#	GOOS=linux GOARCH=arm GOARM=7 CGO_ENABLED=0 go build -ldflags="-w -s" -o build/armv7/coredns .
	GOOS=linux GOARCH=arm64  CGO_ENABLED=0 go build -ldflags="-w -s" -o build/arm64/coredns .
	GOOS=linux GOARCH=amd64  CGO_ENABLED=0 go build -ldflags="-w -s" -o build/amd64/coredns .
	docker buildx build --platform linux/arm64,linux/amd64  -t ghcr.io/skateco/coredns:latest --push .
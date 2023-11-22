FROM golang:1.21 as builder

WORKDIR /app/
ADD . .
RUN CGO_ENABLED=0 go build -ldflags="-w -s" -o coredns main.go

FROM gcr.io/distroless/static

COPY --from=builder --chmod=755 /app/coredns /coredns
COPY docker-entrypoint.sh /docker-entrypoint.sh
USER nonroot:nonroot


EXPOSE 53 53/udp
ENTRYPOINT ["/coredns"]
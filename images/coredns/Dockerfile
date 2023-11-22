FROM golang:1.21 as builder

WORKDIR /app/
ADD . .
RUN CGO_ENABLED=0 go build -ldflags="-w -s" -o coredns main.go

FROM scratch

COPY --from=builder --chmod=755 /app/coredns /coredns


EXPOSE 53 53/udp
ENTRYPOINT ["/coredns"]
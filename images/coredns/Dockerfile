FROM alpine:3.18
ARG TARGETARCH
ARG TARGETVARIANT

COPY ./build/$TARGETARCH$TARGETVARIANT/coredns /coredns

COPY --chmod=755 ./docker-entrypoint.sh /

EXPOSE 53 53/udp
ENTRYPOINT [ "/docker-entrypoint.sh" ]
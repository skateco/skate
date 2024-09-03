FROM openresty/openresty:1.25.3.2-0-bookworm-fat

MAINTAINER Donal Byrne <byrnedo@tcd.ie>

ENV RESTY_ROOT=/usr/local/openresty

RUN DEBIAN_FRONTEND=noninteractive \
    apt-get update -qq && \
    ( curl -f --location https://github.com/Orange-OpenSource/hurl/releases/download/2.0.1/hurl_2.0.1_$(dpkg --print-architecture).deb --output "/tmp/hurl.deb" && \
    DEBIAN_FRONTEND=noninteractive apt install -y /tmp/hurl.deb) || true && \
    DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends \
    ca-certificates \
    build-essential \
    curl \
    unzip \
    make \
    python3 \
    bsdmainutils && \
    rm -rf /var/lib/apt/lists/* && \
    curl -L https://luarocks.org/releases/luarocks-3.11.1.tar.gz --output /tmp/luarocks-3.11.1.tar.gz && \
        cd /tmp && \
        tar -xzvf luarocks-3.11.1.tar.gz && \
        cd luarocks-3.11.1/ && \
        ./configure --prefix=/usr/local/openresty/luajit \
            --with-lua=/usr/local/openresty/luajit/ \
            --lua-suffix=jit \
            --with-lua-include=/usr/local/openresty/luajit/include/luajit-2.1 && \
        make && \
        make install && \
    DEBIAN_FRONTEND=noninteractive /usr/local/openresty/luajit/bin/luarocks install lua-resty-auto-ssl $RESTY_AUTO_SSL_VERSION && \
    opm get ledgetech/lua-resty-http && \
    opm get bungle/lua-resty-template && \
    mkdir /etc/resty-auto-ssl && \
    chown nobody /etc/resty-auto-ssl && \
    curl -L https://raw.githubusercontent.com/slomkowski/nginx-config-formatter/master/nginxfmt.py > /usr/local/bin/nginxfmt && \
    chmod +x /usr/local/bin/nginxfmt && \
    curl -L https://raw.githubusercontent.com/dehydrated-io/dehydrated/v0.7.1/dehydrated > /usr/local/openresty/luajit/bin/resty-auto-ssl/dehydrated && \
    chmod +x /usr/local/openresty/luajit/bin/resty-auto-ssl/dehydrated

COPY docker_entrypoint.sh /docker_entrypoint.sh

COPY error.html /etc/nginx-ingress/
COPY nginx.conf.tmpl /etc/nginx-ingress/nginx.conf.tmpl
COPY service.conf.tmpl /etc/nginx-ingress/service.conf.tmpl


COPY lua $RESTY_ROOT/nginx/lua


ENTRYPOINT ["/docker_entrypoint.sh"]

#!/bin/bash
set -eu

# the path openresty will look for the nginx config
CONF_DIR="/var/lib/skate/ingress/"
CONF_FILE_PATH="${CONF_DIR}nginx.conf"

# copy templates so they're accessible to skate
cp /etc/nginx-ingress/nginx.conf.tmpl $CONF_DIR
cp /etc/nginx-ingress/service.conf.tmpl $CONF_DIR

if [ ! -f "$CONF_DIR"mime.types ]; then
    cp /etc/openresty/mime.types ${CONF_DIR}mime.types
fi

if [ ! -f "$CONF_FILE_PATH" ]; then
    cp /etc/openresty/nginx.conf $CONF_FILE_PATH
fi

pidfile=/usr/local/openresty/nginx/logs/nginx.pid

ensure_self_signed() {
    domain=$1
    ssl_path="/usr/local/openresty/nginx/ssl/${domain}"
    if [ ! -d "$ssl_path" ]; then
        echo "generating self signed cert for $domain"
        mkdir -p $ssl_path
        country=SE
        state=Kalmar
        city=Kalmar
        # This line generates a self signed SSL certificate and key without user intervention.
        openssl req -x509 -newkey rsa:4096 -keyout "${ssl_path}/server.key" -out "${ssl_path}/server.crt" \
            -days 365 -nodes -subj "/C=$country/ST=$state/L=$city/O=Internet/OU=./CN=$domain/emailAddress=postmaster@$domain"
    fi
}

kill_child() {
    pid="$(cat $pidfile 2>/dev/null || echo '')"
    if [ -n "${pid:-}" ]; then
        echo "killing child pid $pid"
        kill -QUIT "$pid"
        wait "$pid"
    fi
}

trap 'echo kill signal received; kill_child' INT TERM QUIT

## Chown storage of ssl certs
mkdir -p /etc/resty-auto-ssl/storage
chown -R nobody /etc/resty-auto-ssl/storage

## Hopefully fix bug
rm -f auto-ssl-sockproc.pid

SYSTEM_RESOLVER=$(cat /etc/resolv.conf | grep -im 1 '^nameserver' | cut -d ' ' -f2)
export SYSTEM_RESOLVER

prepare_config() {
    # ensure self signed cert exists for base url
    domains=$(grep "# anchor::domain" "$CONF_FILE_PATH" | awk '{print $3}' | sort -u)
    for domain in $domains; do
        ensure_self_signed $domain
    done

    # Format it
    echo "formatting..."
    nginxfmt -v $CONF_FILE_PATH

    # Test config
    echo "testing config..."
    if ! /usr/local/openresty/bin/openresty -c $CONF_FILE_PATH -t; then
        cat --number $CONF_FILE_PATH
        # restore prev config
        mv ${CONF_FILE_PATH}.old $CONF_FILE_PATH
    fi

}

# hack to wait for pid to appear
wait_file_changed() {
    tail -fn0 "$1" | head -n1 >/dev/null 2>&1
}

reload_and_wait() {
    # lock
    if {
        set -C
        2>/dev/null >/tmp/ingressreload.lock
    }; then
        # have lock
        pid="$(cat $pidfile 2>/dev/null || echo '')"
        if [ -z "${pid:-}" ]; then
            return
        fi

        prepare_config
        echo "sending HUP..."
        kill -HUP "$pid"
        # release
        rm /tmp/ingressreload.lock
        echo "waiting on $pid"
        wait "$pid"
    fi
}

prepare_config

echo "staring daemon..."
/usr/local/openresty/bin/openresty -c $CONF_FILE_PATH -g "daemon off;" &

trap 'reload_and_wait' HUP

echo 'waiting for pid to appear...'
wait_file_changed $pidfile
pid="$(cat $pidfile)"
echo "master process pid found ($pid)"

echo "waiting on process"
wait $pid

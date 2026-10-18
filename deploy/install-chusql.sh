#!/bin/sh
set -eu
install -d -m 0755 /etc/apt/keyrings
# Use the build host's loopback interface for a colocated Gitea.
for origin in http://127.0.0.1:3000 http://repo.chunana.cn; do
    base="$origin/api/packages/ChuNa7a/debian"
    echo "ChuSQL: trying $base"
    rm -f /var/lib/apt/lists/* /etc/apt/sources.list.d/chusql.list
    if curl -fsSL --connect-timeout 5 --max-time 30 "$base/repository.key" \
        -o /etc/apt/keyrings/gitea-chuna7a.asc; then
        printf 'deb [signed-by=/etc/apt/keyrings/gitea-chuna7a.asc] %s stable main\n' "$base" \
            > /etc/apt/sources.list.d/chusql.list
        if apt-get -o Acquire::http::Timeout=30 -o APT::Update::Error-Mode=any update \
            && apt-get -o Acquire::http::Timeout=30 install -y --no-install-recommends chusql; then
            rm -rf /var/lib/apt/lists/* /var/lib/chusql/* /var/lib/chusql/.[!.]*
            rm -f /etc/chusql/admin-password
            exit 0
        fi
    fi
    echo "ChuSQL: $origin failed; trying next source" >&2
done
exit 1

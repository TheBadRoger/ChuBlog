#!/bin/sh
set -eu
if [ ! -d /var/lib/chusql/system ]; then
    printf '%s\n' "$CHUSQL_ROOT_PASSWORD" | csql-bootstrap --config /etc/chusql/settings.toml --user root --password-stdin
fi
unset CHUSQL_ROOT_PASSWORD
exec chusql-server --config /etc/chusql/settings.toml --listen-host 0.0.0.0 --port 7777

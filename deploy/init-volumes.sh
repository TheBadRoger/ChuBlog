#!/bin/sh
set -eu
mkdir -p /data /auth /srv/ingress /db-data /db-logs
chown 10001:10002 /data /auth
chmod 700 /data
chmod 755 /auth
chown 10002:10002 /srv/ingress
chmod 750 /srv/ingress
chown 10003:10003 /db-data /db-logs
# Per-author repositories are created lazily by the authenticated SSH gateway.
mkdir -p /srv/ingress/authors
chown 10002:10002 /srv/ingress/authors
chmod 750 /srv/ingress/authors

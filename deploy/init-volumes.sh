#!/bin/sh
set -eu
mkdir -p /data /srv/ingress /db-data /db-logs
chown 10001:10002 /data
chmod 700 /data
chown -R 10001:10002 /srv/ingress
chmod 750 /srv/ingress
chown 10003:10003 /db-data /db-logs
# Per-author repositories are created lazily by the authenticated HTTPS gateway.
mkdir -p /srv/ingress/authors
chown 10001:10002 /srv/ingress/authors
chmod 750 /srv/ingress/authors

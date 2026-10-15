#!/bin/sh
set -eu
mkdir -p /data /auth /srv/ingress /db-data /db-logs
chown 10001:10002 /data /auth
chmod 700 /data
chmod 755 /auth
chown 10002:10002 /srv/ingress
chmod 750 /srv/ingress
chown 10003:10003 /db-data /db-logs
if [ ! -d /srv/ingress/content.git ]; then
    setpriv --reuid=10002 --regid=10002 --clear-groups git init --bare --initial-branch=main --shared=group /srv/ingress/content.git
    setpriv --reuid=10002 --regid=10002 --clear-groups git -C /srv/ingress/content.git config receive.denyNonFastForwards true
    setpriv --reuid=10002 --regid=10002 --clear-groups git -C /srv/ingress/content.git config receive.denyDeletes true
fi

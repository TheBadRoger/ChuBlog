#!/bin/sh
set -eu
for origin in http://127.0.0.1:3000 http://repo.chunana.cn https://github.com; do
    if [ "$origin" = https://github.com ]; then
        script=https://raw.githubusercontent.com/TheBadRoger/ChuSQL/main/scripts/install.sh
        api=https://api.github.com/repos/TheBadRoger/ChuSQL/releases/latest
        repo=TheBadRoger/ChuSQL
    else
        script="$origin/ChuNa7a/ChuSQL/raw/branch/main/scripts/install.sh"
        api="$origin/api/v1/repos/ChuNa7a/ChuSQL/releases/latest"
        repo=ChuNa7a/ChuSQL
    fi
    echo "ChuSQL: trying installer at $origin"
    # Explicit archive URL prevents the upstream installer from changing sites.
    if curl -fsSL --connect-timeout 5 --max-time 30 "$script" -o /tmp/chusql-install.sh \
        && curl -fsSL --connect-timeout 5 --max-time 30 "$api" -o /tmp/chusql-release.json; then
        if archive=$(python3 - "$origin/$repo" <<'PY'
import json
import platform
import sys
from urllib.parse import quote
with open('/tmp/chusql-release.json') as stream:
    release = json.load(stream)
arch = {'x86_64': 'x86_64', 'aarch64': 'arm64'}.get(platform.machine(), platform.machine())
names = {asset['name'] for asset in release['assets']}
for name in [f'chusql-linux-{arch}.tar.gz', 'chusql-linux.tar.gz', 'chusql.tar.gz']:
    if name in names:
        print(f'{sys.argv[1]}/releases/download/{quote(release["tag_name"], safe="")}/{name}')
        break
else:
    sys.exit('No matching release archive')
PY
        ); then
            # Each attempt starts with empty build-time state, never deployment volumes.
            rm -rf /usr/lib/chusql /var/lib/chusql /var/log/chusql
            rm -f /etc/chusql/settings.toml /etc/chusql/admin-password
            if CHUSQL_ADMIN_PASSWORD=build-only-unused-password \
                LD_LIBRARY_PATH=/usr/lib/chusql/bin \
                sh /tmp/chusql-install.sh --system --component cli --low-performance-adapted --no-service --no-start --url "$archive"; then
                rm -rf /var/lib/chusql/* /var/lib/chusql/.[!.]*
                rm -f /etc/chusql/admin-password /tmp/chusql-install.sh /tmp/chusql-release.json
                exit 0
            fi
        fi
    fi
    echo "ChuSQL: installer at $origin failed; trying next source" >&2
done
exit 1

#!/bin/sh
# Fetch/build external dependencies as deploy, then start Docker Compose as root.
set -eu
PROJECT_ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
if [ "$(id -u)" -ne 0 ]; then
    echo 'Run with sudo; Git/compilation run as deploy, Docker runs as root.' >&2
    exit 1
fi
sudo -H -u deploy python3 "$PROJECT_ROOT/scripts/init-deps.py"
sudo -H -u deploy python3 "$PROJECT_ROOT/scripts/prepare-deploy.py" --build-database
docker compose --env-file "$PROJECT_ROOT/.env" -f "$PROJECT_ROOT/deploy/compose.yaml" up -d --build
printf '%s\n' 'When the database is ready, run the documented db-init command once.'

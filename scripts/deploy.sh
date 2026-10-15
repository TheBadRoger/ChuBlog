#!/bin/sh
# Initialize dependencies via GitHub HTTPS with self-hosted HTTPS fallback, then build Docker Compose.
set -eu
PROJECT_ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
python3 "$PROJECT_ROOT/scripts/init-deps.py"
python3 "$PROJECT_ROOT/scripts/prepare-deploy.py" --build-database
docker compose --env-file "$PROJECT_ROOT/.env" -f "$PROJECT_ROOT/deploy/compose.yaml" up -d --build
printf '%s\n' 'When the database is ready, run the documented db-init command once.'

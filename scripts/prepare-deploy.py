#!/usr/bin/env python3
"""Build deployment images inside Docker; no host Rust or Haskell tools required."""
import argparse
import os
from pathlib import Path
import shutil
import subprocess
import sys

root = Path(__file__).resolve().parents[1]
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--env-file', type=Path, help='Compose environment file (default: .env, or .env.example when absent)')
parser.add_argument('--build-database', action='store_true', help='compatibility option; database is always built inside Docker')
parser.add_argument('--sources-env-file', type=Path, help='optional second Compose env file overriding build download sources')
parser.add_argument('--project-name', help='Compose project name')
parser.add_argument('--with-cli', action='store_true', help='build and install the ChuSQL CLI in the database image')
args = parser.parse_args()
if shutil.which('docker') is None:
    parser.error('Docker with the Compose plugin is required; see docs/deployment.md.')
for manifest in ['external/ChuSQL-RustDriver/Cargo.toml']:
    if not (root / manifest).is_file():
        parser.error('Initialize external dependencies with python3 scripts/fetch-deps.py first.')
envfile = args.env_file or (root / '.env' if (root / '.env').is_file() else root / '.env.example')
if not envfile.is_file():
    parser.error(f'Environment file does not exist: {envfile}')
command = ['docker', 'compose', '--env-file', str(envfile.resolve())]
if args.sources_env_file:
    if not args.sources_env_file.is_file():
        parser.error(f'Source configuration does not exist: {args.sources_env_file}')
    command.extend(['--env-file', str(args.sources_env_file.resolve())])
if args.project_name:
    command.extend(['-p', args.project_name])
command.extend([
    '-f', str(root / 'deploy/compose.yaml'),
    'build',
])
# Build-only parsing needs values even before deploy.sh creates runtime credentials.
build_env = os.environ.copy()
build_env['CHUSQL_WITH_CLI'] = 'true' if args.with_cli else 'false'
for key in ['CHUSQL_ROOT_PASSWORD', 'CHUSQL_PASSWORD']:
    build_env[key] = build_env.get(key) or 'unused-build-placeholder'
try:
    subprocess.run(command + ['init', 'api', 'db-init', 'web'], cwd=root, check=True, env=build_env)
    for target in ['database', 'database-script', 'database-source']:
        if target == 'database-source':
            print('ChuSQL 发行包安装失败，获取固定版本源码并在 Docker 内构建。', flush=True)
            subprocess.run([sys.executable, str(root / 'scripts/fetch-deps.py'), '--only', 'ChuSQL'],
                           cwd=root, check=True)
        build_env['CHUSQL_BUILD_TARGET'] = target
        result = subprocess.run(command + ['db'], cwd=root, env=build_env)
        if result.returncode == 0:
            break
    else:
        raise subprocess.CalledProcessError(result.returncode, command + ['db'])
except subprocess.CalledProcessError as error:
    raise SystemExit(error.returncode) from None
print('Deployment images built inside Docker. No host binaries were created.')

#!/usr/bin/env python3
"""Build deployment images inside Docker; no host Rust or Haskell tools required."""
import argparse
from pathlib import Path
import shutil
import subprocess

root = Path(__file__).resolve().parents[1]
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--env-file', type=Path, help='Compose environment file (default: .env, or .env.example when absent)')
parser.add_argument('--build-database', action='store_true', help='compatibility option; database is always built inside Docker')
parser.add_argument('--sources-env-file', type=Path, help='optional second Compose env file overriding build download sources')
args = parser.parse_args()
if shutil.which('docker') is None:
    parser.error('Docker with the Compose plugin is required; see docs/deployment.md.')
for manifest in ['external/ChuSQL-RustDriver/Cargo.toml', 'external/ChuSQL/chusql-core/storage/Cargo.toml']:
    if not (root / manifest).is_file():
        parser.error('Initialize external dependencies with python3 scripts/init-deps.py first.')
envfile = args.env_file or (root / '.env' if (root / '.env').is_file() else root / '.env.example')
if not envfile.is_file():
    parser.error(f'Environment file does not exist: {envfile}')
command = ['docker', 'compose', '--env-file', str(envfile.resolve())]
if args.sources_env_file:
    if not args.sources_env_file.is_file():
        parser.error(f'Source configuration does not exist: {args.sources_env_file}')
    command.extend(['--env-file', str(args.sources_env_file.resolve())])
command.extend([
    '-f', str(root / 'deploy/compose.yaml'),
    'build', 'init', 'api', 'ssh', 'db', 'db-init', 'web',
])
try:
    subprocess.run(command, cwd=root, check=True)
except subprocess.CalledProcessError as error:
    raise SystemExit(error.returncode) from None
print('Deployment images built inside Docker. No host binaries were created.')

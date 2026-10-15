#!/usr/bin/env python3
"""Build the API and stage external ChuSQL artifacts without committing dependencies."""
import argparse
from pathlib import Path
import shutil
import subprocess

root = Path(__file__).resolve().parents[1]
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--no-build', action='store_true', help='use an existing API release binary')
parser.add_argument('--chusql-source', type=Path, default=root / 'external/ChuSQL', help='initialized/built external ChuSQL checkout')
parser.add_argument('--build-database', action='store_true', help='build external database artifacts before staging')
args = parser.parse_args()
chusql = args.chusql_source.resolve()
if not (root / 'external/ChuSQL-RustDriver/Cargo.toml').is_file():
    parser.error('Initialize external dependencies with scripts/init-deps.py first.')
if not (chusql / 'chusql-core/storage/Cargo.toml').is_file():
    parser.error(f'ChuSQL source is missing: {chusql}; initialize dependencies first.')
if not args.no_build:
    subprocess.run(['cargo', 'build', '--locked', '--release'], cwd=root, check=True)
if args.build_database:
    subprocess.run(['cargo', 'build', '--locked', '--release'], cwd=chusql / 'chusql-core/storage', check=True)
    for package in ['chusql-server', 'chusql-bootstrap']:
        subprocess.run(['stack', 'build', '--fast'], cwd=chusql / package, check=True)
out = root / 'deploy/bin'
out.mkdir(parents=True, exist_ok=True)
shutil.copy2(root / 'target/release/chublog-api', out / 'chublog-api')
for package, executable in [('chusql-server', 'chusql-server'), ('chusql-bootstrap', 'csql-bootstrap')]:
    matches = list((chusql / package / '.stack-work/install').glob(f'*/**/bin/{executable}'))
    if len(matches) != 1:
        raise SystemExit(f'Expected one built {executable}, found {len(matches)}; use --build-database or select a built --chusql-source.')
    shutil.copy2(matches[0], out / executable)
shutil.copy2(chusql / 'chusql-core/storage/target/release/libchusql_core_storage.so', out)
print('Prepared ignored deploy/bin. Build and run Compose on the same Debian 13 architecture.')

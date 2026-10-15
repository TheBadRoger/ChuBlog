#!/usr/bin/env python3
"""Build the API and copy existing local ChuSQL release artifacts into the Docker context."""
import argparse
from pathlib import Path
import shutil
import subprocess

root = Path(__file__).resolve().parents[1]
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--no-build', action='store_true', help='use an existing API release binary')
args = parser.parse_args()
if not args.no_build:
    subprocess.run(['cargo', 'build', '--locked', '--release'], cwd=root, check=True)
out = root / 'deploy' / 'bin'
out.mkdir(parents=True, exist_ok=True)
shutil.copy2(root / 'target/release/chublog-api', out / 'chublog-api')
chusql = root.parent / 'ChuSQL'
for package, executable in [('chusql-server', 'chusql-server'), ('chusql-bootstrap', 'csql-bootstrap')]:
    matches = list((chusql / package / '.stack-work/install').glob(f'*/**/bin/{executable}'))
    if len(matches) != 1:
        raise SystemExit(f'Expected one built {executable}, found {len(matches)}; build {package} first using its stack.yaml.')
    shutil.copy2(matches[0], out / executable)
shutil.copy2(chusql / 'chusql-core/storage/target/release/libchusql_core_storage.so', out)
print('Prepared deploy/bin. Build and run Compose from this same Linux architecture (Debian 13).')

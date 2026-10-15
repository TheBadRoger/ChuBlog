#!/usr/bin/env python3
"""Initialize pinned external submodules, preferring local repositories over SSH."""
import argparse
from pathlib import Path
import subprocess

ROOT = Path(__file__).resolve().parents[1]
PROJECTS = ['ChuSQL', 'ChuSQL-RustDriver']
PREFIX = 'ChuGit:/srv/git/'


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--database-repo', type=Path, help='local clone/bare repository of ChuSQL')
    parser.add_argument('--driver-repo', type=Path, help='local clone/bare repository of ChuSQL-RustDriver')
    args = parser.parse_args()
    git_args = []
    local_config = ROOT / 'config/git-local.config'
    if not args.database_repo and not args.driver_repo and all((Path('/srv/git') / f'{name}.git').is_dir() for name in PROJECTS):
        current = subprocess.run(['git', 'config', '--local', '--get-all', 'include.path'], cwd=ROOT, text=True, capture_output=True, check=False)
        if str(local_config) not in current.stdout.splitlines():
            subprocess.run(['git', 'config', '--local', '--add', 'include.path', str(local_config)], cwd=ROOT, check=True)
        git_args.extend(['-c', f'include.path={local_config}'])
    for name, override in zip(PROJECTS, [args.database_repo, args.driver_repo]):
        source = override or Path('/srv/git') / f'{name}.git'
        if override and not source.is_dir():
            parser.error(f'Local repository does not exist: {source}')
        if source.is_dir():
            source = source.resolve()
            subprocess.run(['git', '-C', str(source), 'rev-parse', '--git-dir'], check=True, capture_output=True)
            key = f'url.{source.as_uri()}.insteadOf'
            url = f'{PREFIX}{name}.git'
            subprocess.run(['git', 'config', '--local', '--replace-all', key, url], cwd=ROOT, check=True)
            git_args.extend(['-c', f'{key}={url}'])
            print(f'{name}: using local repository {source}', flush=True)
    # The file protocol is enabled only for this explicit, fixed set of dependencies.
    if git_args:
        git_args.extend(['-c', 'protocol.file.allow=always'])
    subprocess.run(['git', *git_args, 'submodule', 'update', '--init', '--', *[f'external/{name}' for name in PROJECTS]], cwd=ROOT, check=True)


if __name__ == '__main__':
    main()

#!/usr/bin/env python3
"""Initialize pinned dependencies over GitHub HTTPS; optionally prefer authorized SSH."""
import argparse
import os
from pathlib import Path
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[1]
PROJECTS = ['ChuSQL', 'ChuSQL-RustDriver']


def git(*args, **kwargs):
    return subprocess.run(['git', *args], cwd=ROOT, **kwargs)


def initialize(name, override, prefer_ssh=False):
    path = f'external/{name}'
    checkout = ROOT / path
    pin = git('ls-files', '--stage', '--', path, text=True, capture_output=True, check=True).stdout.split()
    if len(pin) != 4 or pin[0] != '160000':
        raise RuntimeError(f'{path} is missing its committed submodule pin')
    if (checkout / '.git').exists():
        dirty = git('-C', str(checkout), 'status', '--porcelain', text=True, capture_output=True, check=True).stdout
        if dirty:
            raise RuntimeError(f'{path} has local changes; refusing to overwrite them')
        current = git('-C', str(checkout), 'rev-parse', '--verify', 'HEAD', text=True, capture_output=True, check=False)
        if current.returncode == 0 and current.stdout.strip() == pin[1]:
            print(f'{name}: pinned checkout already available', flush=True)
            return
    if override:
        source = override.resolve()
        if not source.is_dir():
            raise RuntimeError(f'Local repository does not exist: {source}')
        sources = [source.as_uri()]
    else:
        sources = [f'https://github.com/TheBadRoger/{name}.git']
        if prefer_ssh:
            sources.insert(0, f'ChuGit:/srv/git/{name}.git')
    transport_env = {**os.environ, 'GIT_TERMINAL_PROMPT': '0'}
    configured_ssh = git('config', '--get', 'core.sshCommand', capture_output=True, check=False)
    if configured_ssh.returncode != 0 and not any(name in transport_env for name in ['GIT_SSH', 'GIT_SSH_COMMAND']):
        transport_env['GIT_SSH_COMMAND'] = 'ssh -o BatchMode=yes -o ConnectTimeout=10 -o ConnectionAttempts=1'
    for url in sources:
        print(f'{name}: trying {url}', flush=True)
        git('config', '--local', f'submodule.{path}.url', url, check=True)
        options = ['-c', 'protocol.file.allow=always'] if override else []
        if (checkout / '.git').exists():
            git('-C', str(checkout), 'remote', 'set-url', 'origin', url, check=True)
            result = git(*options, '-C', str(checkout), 'fetch', '--no-tags', 'origin', pin[1], env=transport_env, check=False)
            if result.returncode == 0:
                git('-C', str(checkout), 'checkout', '--detach', pin[1], check=True)
                return
        else:
            result = git(*options, 'submodule', 'update', '--init', '--checkout', '--', path, env=transport_env, check=False)
            if result.returncode == 0:
                return
        print(f'{name}: source failed; trying next configured source', file=sys.stderr, flush=True)
    raise RuntimeError(f'No source contains the pinned {name} commit {pin[1]}')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--database-repo', type=Path, help='explicit local development source for ChuSQL')
    parser.add_argument('--driver-repo', type=Path, help='explicit local development source for the driver')
    parser.add_argument('--prefer-ssh', action='store_true', help='authorized devices: try ChuGit SSH first, then GitHub HTTPS')
    args = parser.parse_args()
    for name, override in zip(PROJECTS, [args.database_repo, args.driver_repo]):
        initialize(name, override, args.prefer_ssh)


if __name__ == '__main__':
    main()

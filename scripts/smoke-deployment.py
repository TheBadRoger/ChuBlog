#!/usr/bin/env python3
"""Exercise the one-command blog deployment and uninstall on isolated Docker resources."""
import argparse
import http.cookiejar
import json
import os
from pathlib import Path
import secrets
import subprocess
import tempfile
import urllib.error
import urllib.request

ROOT = Path(__file__).resolve().parents[1]
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--sources-env-file', type=Path, default=ROOT / 'deploy/sources.public.env')
args = parser.parse_args()
project = 'chublog-lifecycle-' + secrets.token_hex(4)
origin = 'http://localhost:18080'
env = {key: value for key, value in os.environ.items()
       if key not in ['COMPOSE_PROFILES', 'COMPOSE_PROJECT_NAME', 'PUBLIC_ORIGIN', 'SITE_ADDRESS',
                      'CHUSQL_ROOT_PASSWORD', 'CHUSQL_PASSWORD', 'HTTP_PORT', 'HTTPS_PORT',
                      'REGISTRY_PREFIX', 'DEBIAN_MIRROR', 'NPM_REGISTRY', 'CARGO_MIRROR_INDEX']}


def run(command, **kwargs):
    return subprocess.run(command, cwd=ROOT, env=env, check=True, text=True, **kwargs)


def read_page(path):
    with urllib.request.urlopen(origin + path, timeout=10) as response:
        return response.geturl(), response.read().decode()


with tempfile.TemporaryDirectory(prefix='chublog-lifecycle-') as temporary:
    config = Path(temporary) / '.env'
    config.write_text('HTTP_BIND=127.0.0.1\nHTTPS_PORT=18443\nSSH_BIND=127.0.0.1\nSSH_PORT=12222\n')
    options = ['--env-file', str(config), '--project-name', project,
               '--sources-env-file', str(args.sources_env_file.resolve())]
    compose = ['docker', 'compose', '--env-file', str(config), '--env-file',
               str(args.sources_env_file.resolve()), '-p', project, '-f', str(ROOT / 'deploy/compose.yaml')]
    try:
        run([str(ROOT / 'scripts/deploy.sh'), *options, '--site', origin], stdin=subprocess.DEVNULL)
        initial = config.read_text()
        assert 'ADMIN_PASSWORD_HASH' not in initial
        assert config.stat().st_mode & 0o777 == 0o600
        _, html = read_page('/')
        assert '个人博客正在施工中' in html
        assert '阅读文章' in html
        assert '文章' in read_page('/articles')[1]
        assert '登录' in read_page('/login')[1]
        assert json.loads(read_page('/api/health')[1])
        client = urllib.request.build_opener(urllib.request.HTTPCookieProcessor(http.cookiejar.CookieJar()))
        password = secrets.token_urlsafe(24)

        def call(path, data=None):
            request = urllib.request.Request(origin + path,
                data=json.dumps(data).encode() if data is not None else None,
                headers={'Origin': origin, 'Content-Type': 'application/json'})
            with client.open(request, timeout=10) as response:
                return json.load(response)

        assert call('/api/auth/register', {'username': 'first', 'password': password})['role'] == 'reader'
        assert call('/api/auth/login', {'username': 'first', 'password': password})['csrf']
        try:
            call('/api/admin/users')
        except urllib.error.HTTPError as error:
            assert error.code == 403
        else:
            raise AssertionError('First user acquired administrator permissions')
        running = run(compose + ['ps', '--services', '--status', 'running'], capture_output=True).stdout.split()
        assert set(running) == {'db', 'api', 'ssh', 'web', 'caddy'}, running
        run(compose + ['run', '--rm', '--no-deps', '-T', 'db-init'], capture_output=True)
        run([str(ROOT / 'scripts/uninstall.sh'), *options], stdin=subprocess.DEVNULL)
        assert run(['docker', 'volume', 'ls', '-q', '--filter',
                    f'label=com.docker.compose.project={project}'], capture_output=True).stdout.strip()
        run([str(ROOT / 'scripts/deploy.sh'), *options], stdin=subprocess.DEVNULL)
        assert config.read_text() == initial, 'Redeploy rotated credentials'
        assert '个人博客正在施工中' in read_page('/')[1]
        assert call('/api/auth/login', {'username': 'first', 'password': password})['csrf']
        print('PASS: one-command build, DB initialization and full blog without an initial administrator', flush=True)
        print('PASS: reading, registration/login, SSH service and first-user permission boundary', flush=True)
        print('PASS: repeat DB provisioning and data-preserving uninstall/redeploy retain credentials', flush=True)
    finally:
        run([str(ROOT / 'scripts/uninstall.sh'), *options, '--purge'], stdin=subprocess.DEVNULL)
        assert not run(['docker', 'ps', '-aq', '--filter',
                        f'label=com.docker.compose.project={project}'], capture_output=True).stdout.strip()
        assert not run(['docker', 'volume', 'ls', '-q', '--filter',
                        f'label=com.docker.compose.project={project}'], capture_output=True).stdout.strip()
        print('PASS: purge removes only the isolated project containers and volumes', flush=True)

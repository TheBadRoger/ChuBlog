#!/usr/bin/env python3
"""Exercise real Docker, authenticated HTTP Git, HTTP authorization, database outage, and restart recovery.
Uses an isolated Compose project and removes only that project's containers/volumes.
Initializes no dependencies; Docker builds the images. Port 1741 must be free.
"""
import argparse
from datetime import datetime
import base64
import gzip
import ssl
import http.cookiejar
import json
import os
from pathlib import Path
import secrets
import subprocess
import sys
import tempfile
import time
import urllib.error
import urllib.request

ROOT = Path(__file__).resolve().parents[1]
ORIGIN = 'http://localhost:1741'
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--sources-env-file', type=Path, help='Compose build source overrides, e.g. deploy/sources.public.env')
parser.add_argument('--https', action='store_true', help='test via a temporary local Caddy HTTPS proxy with a private test CA')
parser.add_argument('--browser-test', action='store_true', help='also run Python Playwright frontend journeys')
args = parser.parse_args()
TLS_CONTEXT = ssl.create_default_context()
if args.https:
    ORIGIN = 'https://localhost:18443'


def run(args, **kwargs):
    return subprocess.run(args, check=True, text=True, **kwargs)


def wait_for(check, timeout=90):
    end = time.monotonic() + timeout
    last = None
    while time.monotonic() < end:
        try:
            value = check()
            if value:
                return value
        except (OSError, ValueError, urllib.error.HTTPError, AssertionError) as error:
            last = error
        time.sleep(1)
    raise AssertionError(f'Timed out: {last}')


class Client:
    def __init__(self):
        self.opener = urllib.request.build_opener(urllib.request.HTTPCookieProcessor(http.cookiejar.CookieJar()), urllib.request.HTTPSHandler(context=TLS_CONTEXT))
        self.csrf = None

    def call(self, method, path, data=None, expected=200):
        headers = {'Origin': ORIGIN, 'Content-Type': 'application/json'}
        if self.csrf:
            headers['X-CSRF-Token'] = self.csrf
        req = urllib.request.Request(ORIGIN + path, method=method, headers=headers,
                                     data=json.dumps(data).encode() if data is not None else None)
        try:
            response = self.opener.open(req, timeout=10)
        except urllib.error.HTTPError as error:
            assert error.code == expected, (path, error.code, error.read())
            return None
        assert response.status == expected, (path, response.status)
        body = response.read()
        return json.loads(body) if body else None


with tempfile.TemporaryDirectory(prefix='chublog-smoke-') as temporary:
    tmp = Path(temporary)
    password = secrets.token_urlsafe(24)
    envfile = tmp / '.env'
    envfile.write_text(f"CHUSQL_ROOT_PASSWORD={secrets.token_urlsafe(24)}\nCHUSQL_PASSWORD={secrets.token_urlsafe(24)}\n"
                       f"REGISTRY_PREFIX={os.environ.get('REGISTRY_PREFIX', '')}\n"
                       'DEBIAN_MIRROR=mirrors.ustc.edu.cn\nALLOW_REGISTRATION=true\n')
    envfile.chmod(0o600)
    project = 'chublog-test-' + secrets.token_hex(4)
    compose = ['docker', 'compose', '--env-file', str(envfile)]
    if args.sources_env_file:
        if not args.sources_env_file.is_file():
            parser.error(f'Source configuration does not exist: {args.sources_env_file}')
        compose.extend(['--env-file', str(args.sources_env_file.resolve())])
    compose.extend(['-p', project, '-f', str(ROOT / 'deploy/compose.yaml')])
    def database_query(sql):
        # Query the private DB from inside its container; no host database port is opened.
        probe = r'''set -eu
exec 3<>/dev/tcp/127.0.0.1/7777
printf '%s\n' '{"method":"hello","protocol":1}' >&3
read -r reply <&3
printf '%s\n' "{\"method\":\"login\",\"user\":\"root\",\"password\":\"${CHUSQL_ROOT_PASSWORD}\"}" >&3
read -r reply <&3
printf '%s\n' '{"method":"query","sql":"USE chublog"}' >&3
read -r reply <&3
printf '%s\n' "$1" >&3
read -r reply <&3
printf '%s\n' "$reply"
'''
        result = run(compose + ['exec', '-T', 'db', 'bash', '-c', probe, 'sql-probe', json.dumps({'method': 'query', 'sql': sql})], capture_output=True)
        response = json.loads(result.stdout)
        assert response['status'] == 'result', response
        return response['rows']

    def database_articles():
        return database_query("SELECT id, author, article_id, path, created_at, published_at, updated_at, entry, cover FROM blog_articles WHERE author = 'alice'")

    caddy_name = project + '-tls-test'
    try:
        run(compose + ['build', 'init', 'api', 'db', 'db-init', 'web'])
        run(compose + ['up', '-d', '--no-build', 'db'])
        wait_for(lambda: subprocess.run(compose + ['run', '--rm', '--no-deps', '-T', 'db-init'], capture_output=True).returncode == 0)
        run(compose + ['up', '-d', '--no-build'])
        if args.https:
            tls_config = tmp / 'Caddyfile.test'
            tls_config.write_text('{\n auto_https disable_redirects\n}\nhttps://localhost:18443 {\n tls internal\n reverse_proxy 127.0.0.1:1741\n}\n')
            run(['docker', 'run', '-d', '--name', caddy_name, '--network', 'host',
                 '--tmpfs', '/data', '--tmpfs', '/config', '-v', str(tls_config) + ':/etc/caddy/Caddyfile:ro',
                 'public.ecr.aws/docker/library/caddy:2'], capture_output=True)
            wait_for(lambda: subprocess.run(['docker', 'exec', caddy_name, 'test', '-f',
                '/data/caddy/pki/authorities/local/root.crt'], capture_output=True).returncode == 0)
            (tmp / 'test-root.crt').write_text(run(['docker', 'exec', caddy_name, 'cat', '/data/caddy/pki/authorities/local/root.crt'], capture_output=True).stdout)
            TLS_CONTEXT = ssl.create_default_context(cafile=str(tmp / 'test-root.crt'))
        admin = Client()
        wait_for(lambda: admin.call('GET', '/api/health'))
        wait_for(lambda: subprocess.run(compose + ['run', '--rm', 'db-init'], capture_output=True).returncode == 0)
        install_token = run(compose + ['exec', '-T', 'api', 'cat', '/data/setup-token'], capture_output=True).stdout.strip()
        admin.call('POST', '/api/setup', {'token': install_token, 'name': 'Smoke blog', 'description': 'Smoke test', 'username': 'admin', 'nickname': '管理员', 'password': password})
        admin.csrf = admin.call('POST', '/api/auth/login', {'username': 'admin', 'password': password})['csrf']
        wait_for(lambda: Client().call('GET', '/api/posts/admin~publishing')['author'] == 'admin')
        wait_for(lambda: Client().call('GET', '/api/posts/admin~development')['author'] == 'admin')
        owners = database_query("SELECT username, permission_level FROM blog_users WHERE permission_level = 'MASTER'")
        assert owners == [['admin', 'MASTER']], owners
        publisher = Client()
        publisher.call('POST', '/api/auth/register', {'nickname': '测试昵称', 'username': 'alice', 'password': password})
        publisher.csrf = publisher.call('POST', '/api/auth/login', {'username': 'alice', 'password': password})['csrf']
        secure_page = urllib.request.Request('http://localhost:1741/account', headers={
            'Host': 'blog.example.invalid', 'X-Forwarded-Host': 'blog.example.invalid', 'X-Forwarded-Proto': 'https'})
        with urllib.request.urlopen(secure_page, timeout=10) as response:
            assert 'https://blog.example.invalid/git/content.git' in response.read().decode()
        publisher.call('GET', '/api/admin/users', expected=403)
        askpass = tmp / 'askpass'
        askpass.write_text('#!/bin/sh\ncase "$1" in *Username*) printf "%s\\n" "$CHUBLOG_TEST_USER";; *) printf "%s\\n" "$CHUBLOG_TEST_PASSWORD";; esac\n')
        askpass.chmod(0o700)
        env = {**os.environ, 'GIT_CONFIG_GLOBAL': '/dev/null', 'GIT_CONFIG_NOSYSTEM': '1',
               'GIT_TERMINAL_PROMPT': '0', 'GIT_ASKPASS': str(askpass),
               'CHUBLOG_TEST_USER': 'alice', 'CHUBLOG_TEST_PASSWORD': password}
        if args.https: env['GIT_SSL_CAINFO'] = str(tmp / 'test-root.crt')
        remote = ORIGIN + '/git/content.git'
        anonymous = {**env, 'GIT_ASKPASS': '/bin/false'}
        assert subprocess.run(['git', 'ls-remote', remote], env=anonymous, capture_output=True).returncode != 0
        wrong = {**env, 'CHUBLOG_TEST_PASSWORD': 'Incorrect-password-123'}
        assert subprocess.run(['git', 'ls-remote', remote], env=wrong, capture_output=True).returncode != 0
        local = tmp / 'articles'
        run(['git', 'init', '--initial-branch=main', str(local)], capture_output=True)
        run(['git', '-C', str(local), 'config', 'user.name', 'Alice'])
        run(['git', '-C', str(local), 'config', 'user.email', 'alice@example.invalid'])
        run(['git', '-C', str(local), 'remote', 'add', 'origin', remote])
        article = local / 'posts/alice-first/main.md'
        article.parent.mkdir(parents=True)
        original = (ROOT / 'content/posts/welcome/main.md').read_text()
        article.write_text(original)
        manifest = local / 'index.toml'
        original_manifest = (ROOT / 'content/index.toml').read_text().replace('id = "welcome"', 'id = "alice-first"').replace('posts/welcome', 'posts/alice-first')
        manifest.write_text(original_manifest)
        chapter = article.parent / 'chapter.md'
        chapter.write_text((ROOT / 'content/posts/welcome/chapter.md').read_text())

        def commit():
            run(['git', '-C', str(local), 'add', '.'])
            run(['git', '-C', str(local), 'commit', '-m', 'Update article'], capture_output=True)

        def push(success=True, *extra):
            result = subprocess.run(['git', '-C', str(local), 'push', 'origin', *extra] if extra else ['git', '-C', str(local), 'push', '-u', 'origin', 'main'], env=env, capture_output=True, text=True)
            assert (result.returncode == 0) == success, result.stderr
            return result

        commit()
        push(False)
        admin.call('PATCH', '/api/admin/users/alice', {'enabled': True, 'role': 'publisher'})
        push()
        wait_for(lambda: Client().call('GET', '/api/posts/alice~alice-first')['meta']['slug'] == 'alice~alice-first')
        wait_for(lambda: (lambda s: s['indexed_commit'] == s['commit'])(admin.call('GET', '/api/admin/status')), timeout=120)
        metadata_rows = database_articles()
        assert len(metadata_rows) == 1
        assert metadata_rows[0][:4] == ['alice~alice-first', 'alice', 'alice-first', 'posts/alice-first/main.md']
        print('PASS: registration, independent grant, real HTTP Git push, publication, real ChuSQL metadata')
        head = run(['git', '-C', str(local), 'rev-parse', 'HEAD'], capture_output=True).stdout.strip()
        def pkt(text):
            payload = text.encode()
            return f'{len(payload)+4:04x}'.encode() + payload
        negotiation = pkt('want ' + head + '\n') * 40 + b'0000' + pkt('done\n')
        basic = 'Basic ' + base64.b64encode(('alice:' + password).encode()).decode()
        for data, expected in [(gzip.compress(negotiation), 200), (gzip.compress(b'a' * (256 * 1024)), 413), (b'bad gzip', 400)]:
            req = urllib.request.Request(remote + '/git-upload-pack', data=data, headers={
                'Authorization': basic, 'Content-Type': 'application/x-git-upload-pack-request', 'Content-Encoding': 'gzip'})
            try:
                with urllib.request.urlopen(req, context=TLS_CONTEXT, timeout=20) as response:
                    assert response.status == expected
                    assert b'PACK' in response.read()
            except urllib.error.HTTPError as error:
                assert error.code == expected, (error.code, error.read())
        print('PASS: gzip Git negotiation, decompressed size limit and invalid gzip rejection')
        first_article = Client().call('GET', '/api/posts/alice~alice-first')
        assert first_article['author'] == 'alice'
        assert {p['name'] for p in first_article['pages']} == {'main.md', 'chapter.md'}
        assert Client().call('GET', '/api/posts/alice~alice-first/pages/chapter.md')['markdown'] == chapter.read_text()
        original_dates = first_article['meta']
        assert metadata_rows[0][4:7] == [datetime.fromisoformat(original_dates[key].replace('Z', '+00:00')).strftime('%Y-%m-%d %H:%M:%S') for key in ('created_at', 'date', 'updated')]
        assert metadata_rows[0][7:] == ['main.md', '']
        # Malformed TOML is staged and survives restart without replacing the live version.
        manifest.write_text('[[articles')
        article.write_text(original + '\nPending change\n')
        commit()
        push()
        wait_for(lambda: 'alice' in admin.call('GET', '/api/admin/status')['pending'])
        assert Client().call('GET', '/api/posts/alice~alice-first')['markdown'] == original
        run(compose + ['restart', 'api'])
        wait_for(lambda: Client().call('GET', '/api/posts/alice~alice-first')['markdown'] == original)
        admin.csrf = admin.call('POST', '/api/auth/login', {'username': 'admin', 'password': password})['csrf']
        assert 'alice' in admin.call('GET', '/api/admin/status')['pending']
        manifest.write_text(original_manifest)
        commit()
        push()
        wait_for(lambda: 'Pending change' in Client().call('GET', '/api/posts/alice~alice-first')['markdown'])
        updated = Client().call('GET', '/api/posts/alice~alice-first')['meta']
        assert updated['date'] == original_dates['date']
        assert updated['created_at'] == original_dates['created_at']
        assert updated['updated'] > original_dates['updated']
        print('PASS: linked Markdown pages, staged broken TOML across restart, repaired import and server timestamps')
        article.write_text(original)
        commit()
        push()
        # Invalid commits must not advance the remote branch. Reset only this disposable local test clone.
        for kind in ['script', 'symlink', 'traversal', 'certificate', 'gitignore', 'oversize', 'pack']:
            if kind == 'script':
                article.write_text(original + '\n<script>alert(1)</script>\n')
            elif kind == 'symlink':
                article.unlink()
                article.symlink_to('/etc/passwd')
            elif kind == 'traversal':
                article.write_text(original + '\n[escape](../other/main.md)\n')
            elif kind == 'certificate':
                (article.parent / 'certificate.txt').write_text('-----BEGIN CERTIFICATE-----\ninvalid\n')
            elif kind == 'gitignore':
                (local / '.gitignore').write_text('*.pem\n')
            elif kind == 'oversize':
                article.write_text(original + 'x' * (257 * 1024))
            else:
                (local / 'oversized.bin').write_bytes(os.urandom(9 * 1024 * 1024))
            commit()
            push(False)
            run(['git', '-C', str(local), 'reset', '--hard', 'origin/main'], capture_output=True)
        push(False, 'main:refs/heads/other')
        push(False, '--delete', 'main')
        print('PASS: HTML, symlink, cross-article link, disguised certificate, gitignore, oversize, branch and deletion rejected')
        # A second authorized account maps the same remote alias to a separate object store.
        bob = Client()
        bob.call('POST', '/api/auth/register', {'nickname': '测试昵称', 'username': 'bob', 'password': password})
        bob.csrf = bob.call('POST', '/api/auth/login', {'username': 'bob', 'password': password})['csrf']
        admin.call('PATCH', '/api/admin/users/bob', {'enabled': True, 'role': 'publisher'})
        bob_env = {**env, 'CHUBLOG_TEST_USER': 'bob'}
        bob_local = tmp / 'bob-articles'
        run(['git', 'clone', remote, str(bob_local)], env=bob_env, capture_output=True)
        assert not (bob_local / 'posts/alice-first/main.md').exists(), 'Alice content leaked into Bob clone'
        alice_commit = run(['git', '-C', str(local), 'rev-parse', 'HEAD'], capture_output=True).stdout.strip()
        result = subprocess.run(['git', '-C', str(bob_local), 'fetch', 'origin', alice_commit], env=bob_env, capture_output=True)
        assert result.returncode != 0, 'Bob fetched Alice commit by hash'
        result = subprocess.run(['git', 'ls-remote', ORIGIN + '/git/alice.git'], env=bob_env, capture_output=True)
        assert result.returncode != 0, 'Bob selected another repository path'
        print('PASS: separate author clone, inaccessible foreign commit and forbidden repository selection')
        # Removing only an index entry deletes the live article, while retaining its files.
        manifest.write_text('articles = []\n')
        commit()
        push()
        wait_for(lambda: Client().call('GET', '/api/stats')['published_posts'] == 2)
        wait_for(lambda: (lambda s: s['indexed_commit'] == s['commit'])(admin.call('GET', '/api/admin/status')), timeout=120)
        Client().call('GET', '/api/posts/alice~alice-first', expected=404)
        assert database_articles() == [], 'Index removal did not delete database metadata'
        print('PASS: index-only deletion removes HTTP article and database metadata')
        manifest.write_text(original_manifest)
        # A valid index whose entry disappeared denotes no article, not a staged update.
        article.unlink()
        commit()
        push()
        wait_for(lambda: not admin.call('GET', '/api/admin/status')['pending'])
        assert Client().call('GET', '/api/stats')['published_posts'] == 2
        article.write_text(original)
        manifest.write_text(original_manifest.replace('public = true', 'public = false'))
        commit()
        push()
        wait_for(lambda: Client().call('GET', '/api/stats')['published_posts'] == 2)
        Client().call('GET', '/api/posts/alice~alice-first', expected=404)
        manifest.write_text(original_manifest)
        article.write_text(original)
        commit()
        push()
        wait_for(lambda: Client().call('GET', '/api/stats')['published_posts'] == 3)
        run(compose + ['stop', 'db'])
        assert Client().call('GET', '/api/posts/alice~alice-first')['meta']['slug'] == 'alice~alice-first'
        run(compose + ['restart', 'api'])
        wait_for(lambda: Client().call('GET', '/api/posts/alice~alice-first')['meta']['slug'] == 'alice~alice-first')
        admin.call('POST', '/api/auth/login', {'username': 'admin', 'password': password}, expected=503)
        assert subprocess.run(['git', 'ls-remote', remote], env=env, capture_output=True).returncode != 0
        run(compose + ['start', 'db'])
        wait_for(lambda: admin.call('POST', '/api/auth/login', {'username': 'admin', 'password': password}))
        admin.csrf = admin.call('POST', '/api/auth/login', {'username': 'admin', 'password': password})['csrf']
        admin.call('PATCH', '/api/admin/users/alice', {'enabled': True, 'role': 'reader'})
        article.write_text(original + '\nRevoked publisher\n')
        commit()
        push(False)
        wait_for(lambda: (lambda s: s['indexed_commit'] == s['commit'])(admin.call('GET', '/api/admin/status')), timeout=120)
        print('PASS: draft privacy, database outage, API restart recovery, revocation, automatic index recovery')
        bob.csrf = bob.call('POST', '/api/auth/login', {'username': 'bob', 'password': password})['csrf']
        bob.call('PUT', '/api/account/password', {'old_password': password, 'new_password': password + '-new'})
        assert subprocess.run(['git', 'ls-remote', remote], env=bob_env, capture_output=True).returncode != 0
        bob_env['CHUBLOG_TEST_PASSWORD'] = password + '-new'
        run(['git', 'ls-remote', remote], env=bob_env, capture_output=True)
        admin.call('PATCH', '/api/admin/users/bob', {'enabled': False, 'role': 'publisher'})
        assert subprocess.run(['git', 'ls-remote', remote], env=bob_env, capture_output=True).returncode != 0
        print('PASS: anonymous/wrong-password rejection, password rotation and disabled-account Git denial')
        if args.browser_test:
            run([sys.executable, str(ROOT / 'test/test-web.py'), '--origin', ORIGIN], input=password)
    except Exception:
        run(compose + ['logs', '--tail', '40'])
        raise
    finally:
        if args.https: subprocess.run(['docker', 'rm', '-f', caddy_name], capture_output=True)
        run(compose + ['down', '--volumes', '--remove-orphans'])

#!/usr/bin/env python3
"""Exercise real Docker, SSH, Git, HTTP authorization, database outage, and restart recovery.
Uses an isolated Compose project and removes only that project's containers/volumes.
Run prepare-deploy.py first. Ports 18080, 18443, 12222 must be free.
"""
import argparse
import http.cookiejar
import json
import os
from pathlib import Path
import secrets
import subprocess
import tempfile
import time
import urllib.error
import urllib.request

ROOT = Path(__file__).resolve().parents[1]
ORIGIN = 'http://localhost:18080'
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--key-type', choices=['ed25519', 'rsa'], default='ed25519')
args = parser.parse_args()


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
        self.opener = urllib.request.build_opener(urllib.request.HTTPCookieProcessor(http.cookiejar.CookieJar()))
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
    hashed = run([str(ROOT / 'deploy/bin/chublog-api'), 'hash-password'], input=password, capture_output=True).stdout.strip()
    envfile = tmp / '.env'
    envfile.write_text(f"PUBLIC_ORIGIN={ORIGIN}\nSITE_ADDRESS=http://localhost:80\nADMIN_PASSWORD_HASH='{hashed}'\n"
                       f"CHUSQL_ROOT_PASSWORD={secrets.token_urlsafe(24)}\nCHUSQL_PASSWORD={secrets.token_urlsafe(24)}\n"
                       'DEBIAN_MIRROR=mirrors.ustc.edu.cn\nALLOW_REGISTRATION=true\nHTTP_BIND=127.0.0.1\nHTTP_PORT=18080\nHTTPS_PORT=18443\nSSH_BIND=127.0.0.1\nSSH_PORT=12222\n')
    envfile.chmod(0o600)
    project = 'chublog-test-' + secrets.token_hex(4)
    compose = ['docker', 'compose', '--env-file', str(envfile), '-p', project, '-f', str(ROOT / 'deploy/compose.yaml')]
    def database_articles():
        # Query the private DB from inside its container; no host database port is opened.
        probe = r'''set -eu
exec 3<>/dev/tcp/127.0.0.1/7777
printf '%s\n' '{"method":"hello","protocol":1}' >&3
read -r reply <&3
printf '%s\n' "{\"method\":\"login\",\"user\":\"root\",\"password\":\"${CHUSQL_ROOT_PASSWORD}\"}" >&3
read -r reply <&3
printf '%s\n' '{"method":"query","sql":"USE chublog"}' >&3
read -r reply <&3
printf '%s\n' '{"method":"query","sql":"SELECT id, author, article_id, path FROM blog_articles"}' >&3
read -r reply <&3
printf '%s\n' "$reply"
'''
        result = run(compose + ['exec', '-T', 'db', 'bash', '-c', probe], capture_output=True)
        response = json.loads(result.stdout)
        assert response['status'] == 'result', response
        return response['rows']

    try:
        run(compose + ['up', '-d', '--build'])
        admin = Client()
        wait_for(lambda: admin.call('GET', '/api/health'))
        wait_for(lambda: subprocess.run(compose + ['run', '--rm', 'db-init'], capture_output=True).returncode == 0)
        admin.csrf = admin.call('POST', '/api/auth/login', {'username': 'admin', 'password': password})['csrf']
        publisher = Client()
        publisher.call('POST', '/api/auth/register', {'username': 'alice', 'password': password})
        publisher.csrf = publisher.call('POST', '/api/auth/login', {'username': 'alice', 'password': password})['csrf']
        run(['ssh-keygen', '-q', '-t', args.key_type, '-b', '3072', '-N', '', '-f', str(tmp / 'key')])
        publisher.call('PUT', '/api/account/ssh-key', {'public_key': (tmp / 'key.pub').read_text().strip()})
        publisher.call('GET', '/api/admin/users', expected=403)
        env = os.environ.copy()
        env['GIT_SSH_COMMAND'] = f'ssh -i {tmp / "key"} -o IdentitiesOnly=yes -o StrictHostKeyChecking=accept-new -o UserKnownHostsFile={tmp / "known_hosts"} -p 12222'
        local = tmp / 'articles'
        run(['git', 'init', '--initial-branch=main', str(local)], capture_output=True)
        run(['git', '-C', str(local), 'config', 'user.name', 'Alice'])
        run(['git', '-C', str(local), 'config', 'user.email', 'alice@example.invalid'])
        run(['git', '-C', str(local), 'remote', 'add', 'origin', 'git@localhost:content.git'])
        article = local / 'posts/alice-first/index.md'
        article.parent.mkdir(parents=True)
        original = (ROOT / 'content/posts/welcome/index.md').read_text().replace('slug: "welcome"', 'slug: "alice-first"')
        article.write_text(original)
        manifest = local / 'index.txt'
        manifest.write_text('alice-first posts/alice-first index.md\n')

        def commit():
            run(['git', '-C', str(local), 'add', '.'])
            run(['git', '-C', str(local), 'commit', '-m', 'Update article'], capture_output=True)

        def push(success=True, *extra):
            result = subprocess.run(['git', '-C', str(local), 'push', 'origin', *extra] if extra else ['git', '-C', str(local), 'push', '-u', 'origin', 'main'], env=env, capture_output=True, text=True)
            assert (result.returncode == 0) == success, result.stderr
            return result

        commit()
        push(False)
        admin.call('PATCH', '/api/admin/users/alice', {'enabled': True, 'role': 'publisher', 'prefixes': ['alice-']})
        push()
        wait_for(lambda: Client().call('GET', '/api/posts/alice~alice-first')['meta']['slug'] == 'alice~alice-first')
        wait_for(lambda: (lambda s: s['indexed_commit'] == s['commit'])(admin.call('GET', '/api/admin/status')), timeout=120)
        assert database_articles() == [['alice~alice-first', 'alice', 'alice-first', 'posts/alice-first/index.md']]
        print('PASS: registration, independent grant, real SSH push, publication, real ChuSQL metadata')
        ssh = ['ssh', '-i', str(tmp / 'key'), '-o', 'IdentitiesOnly=yes', '-o', 'StrictHostKeyChecking=accept-new', '-o', f'UserKnownHostsFile={tmp / "known_hosts"}', '-p', '12222', 'git@localhost']
        result = subprocess.run(ssh + ['id'], capture_output=True)
        assert result.returncode != 0, 'SSH shell command unexpectedly succeeded'
        # Invalid commits must not advance the remote branch. Reset only this disposable local test clone.
        for kind in ['script', 'symlink', 'manifest', 'certificate', 'gitignore', 'oversize', 'pack']:
            if kind == 'script':
                article.write_text(original + '\n<script>alert(1)</script>\n')
            elif kind == 'symlink':
                article.unlink()
                article.symlink_to('/etc/passwd')
            elif kind == 'manifest':
                manifest.write_text('alice-first posts/missing index.md\n')
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
        print('PASS: arbitrary command, HTML, symlink, invalid index, disguised certificate, gitignore, oversize, branch and deletion rejected')
        # A second authorized key maps the same remote alias to a separate object store.
        bob = Client()
        bob.call('POST', '/api/auth/register', {'username': 'bob', 'password': password})
        bob.csrf = bob.call('POST', '/api/auth/login', {'username': 'bob', 'password': password})['csrf']
        run(['ssh-keygen', '-q', '-t', 'ed25519', '-N', '', '-f', str(tmp / 'bob-key')])
        bob.call('PUT', '/api/account/ssh-key', {'public_key': (tmp / 'bob-key.pub').read_text().strip()})
        admin.call('PATCH', '/api/admin/users/bob', {'enabled': True, 'role': 'publisher'})
        bob_env = env.copy()
        bob_env['GIT_SSH_COMMAND'] = env['GIT_SSH_COMMAND'].replace(str(tmp / 'key'), str(tmp / 'bob-key'))
        bob_local = tmp / 'bob-articles'
        run(['git', 'clone', 'git@localhost:content.git', str(bob_local)], env=bob_env, capture_output=True)
        assert not (bob_local / 'posts/alice-first/index.md').exists(), 'Alice content leaked into Bob clone'
        alice_commit = run(['git', '-C', str(local), 'rev-parse', 'HEAD'], capture_output=True).stdout.strip()
        result = subprocess.run(['git', '-C', str(bob_local), 'fetch', 'origin', alice_commit], env=bob_env, capture_output=True)
        assert result.returncode != 0, 'Bob fetched Alice commit by hash'
        result = subprocess.run(['git', 'ls-remote', 'git@localhost:alice.git'], env=bob_env, capture_output=True)
        assert result.returncode != 0, 'Bob selected another repository path'
        print('PASS: separate author clone, inaccessible foreign commit and forbidden repository selection')
        # Removing only an index entry deletes the live article, while retaining its files.
        manifest.write_text('')
        commit()
        push()
        wait_for(lambda: Client().call('GET', '/api/stats')['published_posts'] == 0)
        wait_for(lambda: (lambda s: s['indexed_commit'] == s['commit'])(admin.call('GET', '/api/admin/status')), timeout=120)
        Client().call('GET', '/api/posts/alice~alice-first', expected=404)
        assert database_articles() == [], 'Index removal did not delete database metadata'
        print('PASS: index-only deletion removes HTTP article and database metadata')
        manifest.write_text('alice-first posts/alice-first index.md\n')
        article.write_text(original.replace('status: "published"', 'status: "draft"'))
        commit()
        push()
        wait_for(lambda: Client().call('GET', '/api/stats')['published_posts'] == 0)
        Client().call('GET', '/api/posts/alice~alice-first', expected=404)
        article.write_text(original)
        commit()
        push()
        wait_for(lambda: Client().call('GET', '/api/stats')['published_posts'] == 1)
        run(compose + ['stop', 'db'])
        assert Client().call('GET', '/api/posts/alice~alice-first')['meta']['slug'] == 'alice~alice-first'
        run(compose + ['restart', 'api'])
        wait_for(lambda: Client().call('GET', '/api/posts/alice~alice-first')['meta']['slug'] == 'alice~alice-first')
        admin.csrf = admin.call('POST', '/api/auth/login', {'username': 'admin', 'password': password})['csrf']
        admin.call('PATCH', '/api/admin/users/alice', {'enabled': True, 'role': 'reader', 'prefixes': []})
        article.write_text(original + '\nRevoked publisher\n')
        commit()
        push(False)
        run(compose + ['start', 'db'])
        wait_for(lambda: (lambda s: s['indexed_commit'] == s['commit'])(admin.call('GET', '/api/admin/status')), timeout=120)
        print('PASS: draft privacy, database outage, API restart recovery, revocation, automatic index recovery')
    except Exception:
        run(compose + ['logs', '--tail', '40'])
        raise
    finally:
        run(compose + ['down', '--volumes', '--remove-orphans'])

#!/usr/bin/env python3
"""Run account integration tests against an isolated, disposable real ChuSQL container.
Requires a locally built database image and the development Rust toolchain.
"""
import argparse
import json
import os
from pathlib import Path
import secrets
import socket
import subprocess
import time

ROOT = Path(__file__).resolve().parents[1]
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--database-image', default='chublog-db:latest')
args = parser.parse_args()
name = 'chublog-accounts-test-' + secrets.token_hex(4)
password = secrets.token_urlsafe(32)
try:
    subprocess.run(['docker', 'run', '-d', '--name', name,
        '--publish', '127.0.0.1::7777',
        '--tmpfs', '/var/lib/chusql:uid=10003,gid=10003,mode=700',
        '--tmpfs', '/var/log/chusql:uid=10003,gid=10003,mode=700',
        '--env', 'CHUSQL_ROOT_PASSWORD', args.database_image],
        env={**os.environ, 'CHUSQL_ROOT_PASSWORD': password}, check=True, capture_output=True)
    info = json.loads(subprocess.check_output(['docker', 'inspect', name]))[0]
    port = int(info['NetworkSettings']['Ports']['7777/tcp'][0]['HostPort'])
    deadline = time.monotonic() + 60
    while True:
        try:
            with socket.create_connection(('127.0.0.1', port), timeout=3) as connection:
                stream = connection.makefile('rwb')
                for request, status in [({'method': 'hello', 'protocol': 1}, 'hello'),
                                        ({'method': 'login', 'user': 'root', 'password': password}, 'ok')]:
                    stream.write((json.dumps(request) + '\n').encode())
                    stream.flush()
                    if json.loads(stream.readline())['status'] != status:
                        raise OSError('Database authentication not ready')
                break
        except (OSError, ValueError, KeyError):
            if time.monotonic() >= deadline:
                raise RuntimeError('Test database did not become ready')
            time.sleep(.5)
    # Provisioning changes the shared database identity catalog and can invalidate other sessions.
    result = subprocess.run(['cargo', 'test', '--workspace', '--offline', '--', '--include-ignored', '--test-threads=1'],
        cwd=ROOT, env={**os.environ, 'CHUBLOG_TEST_DB_ADDRESS': f'127.0.0.1:{port}',
                       'CHUBLOG_TEST_DB_PASSWORD': password})
    raise SystemExit(result.returncode)
finally:
    subprocess.run(['docker', 'rm', '-f', name], capture_output=True)

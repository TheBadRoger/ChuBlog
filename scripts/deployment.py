#!/usr/bin/env python3
"""Deploy the blog, update it, or uninstall its Compose project."""
import argparse
import os
from pathlib import Path
import secrets
import shutil
import subprocess
import sys
import tempfile
import time

ROOT = Path(__file__).resolve().parents[1]
DB_KEYS = ('CHUSQL_ROOT_PASSWORD', 'CHUSQL_PASSWORD')
RETIRED_KEYS = ('ADMIN_USERNAME', 'ADMIN_PASSWORD_HASH', 'PUBLIC_ORIGIN', 'SITE_ADDRESS',
                'SITE_TITLE', 'HTTP_PORT', 'HTTPS_PORT', 'HTTP_BIND')


def read_config(path):
    values = {}
    if path.is_file():
        for line in path.read_text().splitlines():
            key, sep, value = line.partition('=')
            if sep and key.strip() and not key.lstrip().startswith('#'):
                values[key.strip()] = value.strip().strip("'\"")
    return values


def configure(path):
    text = path.read_text() if path.exists() else (ROOT / '.env.example').read_text()
    values = read_config(path if path.exists() else ROOT / '.env.example')
    changes = {}
    for key in DB_KEYS:
        value = os.environ.get(key, values.get(key, ''))
        if key in os.environ and (len(value) < 16 or value.startswith('replace-with-')):
            raise RuntimeError(f'{key} 环境变量无效，请先取消该覆盖或设置至少 16 字节的口令。')
        if not value or value.startswith('replace-with-'):
            changes[key] = secrets.token_urlsafe(32)
        elif len(value) < 16:
            raise RuntimeError(f'{key} 至少需要 16 字节；已有口令不会自动轮换。')
        elif value != values.get(key, ''):
            if values.get(key) and not values[key].startswith('replace-with-'):
                raise RuntimeError(f'{key} 环境覆盖与已保存口令不同，已停止以避免破坏现有数据库访问。')
            changes[key] = value
    effective = {**values, **changes}
    if os.environ.get(DB_KEYS[0], effective.get(DB_KEYS[0])) == os.environ.get(DB_KEYS[1], effective.get(DB_KEYS[1])):
        raise RuntimeError('数据库 root 与博客数据库账号必须使用不同口令。')
    seen = set()
    lines = []
    for line in text.splitlines():
        key = line.partition('=')[0].strip()
        if key in RETIRED_KEYS:
            continue
        if key in changes:
            line = f'{key}={changes[key]}'
            seen.add(key)
        lines.append(line)
    lines.extend(f'{key}={value}' for key, value in changes.items() if key not in seen)
    path.parent.mkdir(parents=True, exist_ok=True)
    fd, temporary = tempfile.mkstemp(prefix=f'.{path.name}.', dir=path.parent)
    try:
        with os.fdopen(fd, 'w') as config:
            os.fchmod(config.fileno(), 0o600)
            config.write('\n'.join(lines) + '\n')
            config.flush()
            os.fsync(config.fileno())
        os.replace(temporary, path)
        directory = os.open(path.parent, os.O_RDONLY)
        try:
            os.fsync(directory)
        finally:
            os.close(directory)
    finally:
        Path(temporary).unlink(missing_ok=True)
    print(f'配置已就绪：{path}（权限 600，已有数据库口令保留）', flush=True)


def run(command, **kwargs):
    return subprocess.run(command, cwd=ROOT, check=True, **kwargs)


def compose_command(args, envfile):
    command = ['docker', 'compose', '--env-file', str(envfile)]
    if args.sources_env_file:
        command.extend(['--env-file', str(args.sources_env_file.resolve())])
    return command + ['-p', args.project_name, '-f', str(ROOT / 'deploy/compose.yaml')]


def deploy(args):
    configure(args.env_file)
    print('获取固定版本的外部依赖…', flush=True)
    run([sys.executable, str(ROOT / 'scripts/fetch-deps.py')])
    print('在 Docker 内构建生产镜像…', flush=True)
    build = [sys.executable, str(ROOT / 'scripts/prepare-deploy.py'),
             '--env-file', str(args.env_file), '--project-name', args.project_name]
    if args.sources_env_file:
        build.extend(['--sources-env-file', str(args.sources_env_file.resolve())])
    run(build)
    compose = compose_command(args, args.env_file)
    print('启动数据库并初始化博客数据库与专用账号…', flush=True)
    run(compose + ['up', '-d', '--no-build', 'db'])
    deadline = time.monotonic() + 120
    last_error = ''
    while time.monotonic() < deadline:
        result = subprocess.run(compose + ['run', '--rm', '--no-deps', '-T', '--entrypoint',
                                'timeout', 'db-init', '15s', 'chublog-api', 'provision-db'],
                                cwd=ROOT, text=True, capture_output=True)
        if result.returncode == 0:
            break
        last_error = result.stderr[-3000:]
        time.sleep(2)
    else:
        values = read_config(args.env_file)
        for key in DB_KEYS:
            for secret in (values.get(key), os.environ.get(key)):
                if secret:
                    last_error = last_error.replace(secret, '***')
        raise RuntimeError('数据库初始化失败，部署未完成。检查数据库日志和已有口令。\n' + last_error)
    print('启动博客与 SSH…', flush=True)
    run(compose + ['up', '-d', '--no-build', '--remove-orphans', '--wait', '--wait-timeout', '90'])
    port = os.environ.get('WEB_PORT', read_config(args.env_file).get('WEB_PORT', '1741'))
    print(f'部署完成：http://127.0.0.1:{port}\n不创建初始管理员；注册账户默认为普通用户。', flush=True)


def update(args):
    if run(['git', 'status', '--porcelain'], capture_output=True, text=True).stdout.strip():
        raise RuntimeError('工作区有未提交修改，更新已停止；先提交或自行保存修改。')
    run(['git', 'pull', '--ff-only'])
    # Reload the updated deployment code instead of running an old in-memory copy.
    command = [sys.executable, str(ROOT / 'scripts/deployment.py'), 'deploy',
               '--env-file', str(args.env_file), '--project-name', args.project_name]
    if args.sources_env_file:
        command.extend(['--sources-env-file', str(args.sources_env_file.resolve())])
    os.execv(sys.executable, command)


def uninstall(args):
    envfile = args.env_file if args.env_file.is_file() else ROOT / '.env.example'
    env = os.environ.copy()
    values = read_config(envfile)
    # Compose validates these variables even when down does not connect to the DB.
    for key in DB_KEYS:
        env[key] = env.get(key) or values.get(key) or 'unused-uninstall-placeholder'
    command = compose_command(args, envfile) + ['--profile', '*', 'down',
                '--rmi', 'local', '--remove-orphans']
    if args.purge:
        print(f'永久删除项目 {args.project_name} 的数据卷。', flush=True)
        command.append('--volumes')
    run(command, env=env)
    print('卸载完成。' + ('项目数据卷已删除。' if args.purge else '数据卷已保留；彻底删除使用 --purge。'), flush=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('action', choices=['deploy', 'update', 'uninstall'])
    parser.add_argument('--env-file', type=Path, default=ROOT / '.env')
    parser.add_argument('--sources-env-file', type=Path, help='optional public/build source overrides')
    parser.add_argument('--project-name', default='chublog', help='Compose project name (default: chublog)')
    parser.add_argument('--purge', action='store_true', help='uninstall only: permanently delete project volumes')
    args = parser.parse_args()
    if args.purge and args.action != 'uninstall':
        parser.error('--purge 只能用于卸载。')
    if args.sources_env_file and not args.sources_env_file.is_file():
        parser.error('下载源配置文件不存在。')
    if args.sources_env_file:
        supported = {'REGISTRY_PREFIX', 'DEBIAN_MIRROR', 'NPM_REGISTRY', 'CARGO_MIRROR_INDEX'}
        if set(read_config(args.sources_env_file)) - supported:
            parser.error('下载源配置只能包含镜像和软件包来源，不能覆盖站点或数据库口令。')
    args.env_file = args.env_file.resolve()
    if shutil.which('docker') is None:
        parser.error('需要安装 Docker Engine 和 Docker Compose 插件。')
    try:
        {'deploy': deploy, 'update': update, 'uninstall': uninstall}[args.action](args)
    except subprocess.CalledProcessError as error:
        print(f'操作失败，命令退出码：{error.returncode}', file=sys.stderr)
        return error.returncode
    except (OSError, ValueError, RuntimeError) as error:
        print(f'操作失败：{error}', file=sys.stderr)
        return 1
    return 0


if __name__ == '__main__':
    raise SystemExit(main())

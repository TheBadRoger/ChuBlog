#!/usr/bin/env python3
"""Verify pinned submodules initialize from local repositories without an SSH connection."""
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest

SCRIPT = Path(__file__).with_name('fetch-deps.py')


class InitializeDependencies(unittest.TestCase):
    def test_https_default_and_self_hosted_fallback_preserve_pins(self):
        with tempfile.TemporaryDirectory(prefix='chublog deps ') as temporary:
            root = Path(temporary)
            project = root / 'blog'
            project.mkdir()
            (project / 'scripts').mkdir()
            shutil.copy2(SCRIPT, project / 'scripts/fetch-deps.py')
            env = {**os.environ, 'GIT_CONFIG_GLOBAL': '/dev/null', 'GIT_CONFIG_NOSYSTEM': '1'}

            def git(cwd, *args):
                return subprocess.run(['git', *args], cwd=cwd, env=env, check=True, text=True, capture_output=True).stdout.strip()

            git(project, 'init', '--initial-branch=main')
            modules = []
            sources = []
            for name in ['ChuSQL', 'ChuSQL-RustDriver']:
                source = root / name
                source.mkdir()
                git(source, 'init', '--initial-branch=main')
                (source / 'source.txt').write_text(f'{name} committed source')
                git(source, 'add', '.')
                git(source, '-c', 'user.name=Test', '-c', 'user.email=test@example.invalid', 'commit', '-m', 'Dependency version')
                oid = git(source, 'rev-parse', 'HEAD')
                # Dirty source must never leak into the blog or fetched dependency.
                (source / 'source.txt').write_text('uncommitted changes')
                path = f'external/{name}'
                modules.append(f'[submodule "{path}"]\n path = {path}\n url = https://github.com/TheBadRoger/{name}.git\n')
                git(project, 'update-index', '--add', '--cacheinfo', f'160000,{oid},{path}')
                sources.append(str(source))
            (project / '.gitmodules').write_text(''.join(modules))
            git(project, 'add', '.gitmodules')
            subprocess.run(['python3', 'scripts/fetch-deps.py', '--database-repo', sources[0], '--driver-repo', sources[1]], cwd=project, env=env, check=True, capture_output=True)
            for name in ['ChuSQL', 'ChuSQL-RustDriver']:
                self.assertEqual((project / f'external/{name}/source.txt').read_text(), f'{name} committed source')
            self.assertTrue(all(line.startswith('160000') for line in git(project, 'ls-files', '--stage', 'external').splitlines()))
            # A fresh clone must use HTTPS without attempting SSH. Redirect only the test's
            # GitHub remotes to local fixtures, so this verifies fallback without network access.
            fresh = root / 'fresh'
            git(project, '-c', 'user.name=Test', '-c', 'user.email=test@example.invalid', 'commit', '-m', 'Pinned dependencies')
            git(root, 'clone', '--no-local', str(project), str(fresh))
            (fresh / 'scripts').mkdir()
            shutil.copy2(SCRIPT, fresh / 'scripts/fetch-deps.py')
            fallback_env = {**env, 'GIT_ALLOW_PROTOCOL': 'file', 'GIT_CONFIG_COUNT': '2'}
            for i, (name, source) in enumerate(zip(['ChuSQL', 'ChuSQL-RustDriver'], sources)):
                fallback_env[f'GIT_CONFIG_KEY_{i}'] = f'url.{Path(source).as_uri()}.insteadOf'
                fallback_env[f'GIT_CONFIG_VALUE_{i}'] = f'https://github.com/TheBadRoger/{name}.git'
            subprocess.run(['python3', 'scripts/fetch-deps.py'], cwd=fresh, env=fallback_env, check=True, capture_output=True)
            for name in ['ChuSQL', 'ChuSQL-RustDriver']:
                self.assertEqual((fresh / f'external/{name}/source.txt').read_text(), f'{name} committed source')
                self.assertEqual(git(fresh / f'external/{name}', 'config', '--get', 'remote.origin.url'), f'https://github.com/TheBadRoger/{name}.git')
            # An empty GitHub source clones successfully but lacks the pin; the
            # self-hosted HTTPS fallback must recover the partial checkout.
            partial = root / 'partial'
            git(root, 'clone', '--no-local', str(project), str(partial))
            (partial / 'scripts').mkdir()
            shutil.copy2(SCRIPT, partial / 'scripts/fetch-deps.py')
            partial_env = {**env, 'GIT_ALLOW_PROTOCOL': 'file', 'GIT_CONFIG_COUNT': '4'}
            for i, (name, source) in enumerate(zip(['ChuSQL', 'ChuSQL-RustDriver'], sources)):
                empty_source = root / f'empty-{name}.git'
                git(root, 'init', '--bare', str(empty_source))
                partial_env[f'GIT_CONFIG_KEY_{i}'] = f'url.{empty_source.as_uri()}.insteadOf'
                partial_env[f'GIT_CONFIG_VALUE_{i}'] = f'https://github.com/TheBadRoger/{name}.git'
                partial_env[f'GIT_CONFIG_KEY_{i + 2}'] = f'url.{Path(source).as_uri()}.insteadOf'
                partial_env[f'GIT_CONFIG_VALUE_{i + 2}'] = f'https://repo.chunana.cn/ChuNa7a/{name}.git'
            result = subprocess.run(['python3', 'scripts/fetch-deps.py'], cwd=partial, env=partial_env, text=True, capture_output=True)
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
            for name in ['ChuSQL', 'ChuSQL-RustDriver']:
                self.assertEqual((partial / f'external/{name}/source.txt').read_text(), f'{name} committed source')
                self.assertEqual(git(partial / f'external/{name}', 'config', '--get', 'remote.origin.url'), f'https://repo.chunana.cn/ChuNa7a/{name}.git')
            # Running the initialization a second time must preserve the same pins.
            subprocess.run(['python3', 'scripts/fetch-deps.py', '--database-repo', sources[0], '--driver-repo', sources[1]], cwd=project, env=env, check=True, capture_output=True)


if __name__ == '__main__':
    unittest.main()

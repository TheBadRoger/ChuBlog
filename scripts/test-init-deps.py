#!/usr/bin/env python3
"""Verify pinned submodules initialize from local repositories without an SSH connection."""
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest

SCRIPT = Path(__file__).with_name('init-deps.py')


class InitializeDependencies(unittest.TestCase):
    def test_local_rewrite_fetches_only_pinned_dependency_commits(self):
        with tempfile.TemporaryDirectory(prefix='chublog deps ') as temporary:
            root = Path(temporary)
            project = root / 'blog'
            project.mkdir()
            (project / 'scripts').mkdir()
            shutil.copy2(SCRIPT, project / 'scripts/init-deps.py')
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
                modules.append(f'[submodule "{path}"]\n path = {path}\n url = ChuGit:/srv/git/{name}.git\n')
                git(project, 'update-index', '--add', '--cacheinfo', f'160000,{oid},{path}')
                sources.append(str(source))
            (project / '.gitmodules').write_text(''.join(modules))
            git(project, 'add', '.gitmodules')
            subprocess.run(['python3', 'scripts/init-deps.py', '--database-repo', sources[0], '--driver-repo', sources[1]], cwd=project, env=env, check=True, capture_output=True)
            for name in ['ChuSQL', 'ChuSQL-RustDriver']:
                self.assertEqual((project / f'external/{name}/source.txt').read_text(), f'{name} committed source')
            self.assertTrue(all(line.startswith('160000') for line in git(project, 'ls-files', '--stage', 'external').splitlines()))
            # Running the initialization a second time must preserve the same pins.
            subprocess.run(['python3', 'scripts/init-deps.py', '--database-repo', sources[0], '--driver-repo', sources[1]], cwd=project, env=env, check=True, capture_output=True)


if __name__ == '__main__':
    unittest.main()

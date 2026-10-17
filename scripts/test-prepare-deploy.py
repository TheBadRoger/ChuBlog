#!/usr/bin/env python3
"""Check deployment preparation without host compilers or a Docker daemon."""
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest

SCRIPT = Path(__file__).with_name('prepare-deploy.py')


class PrepareDeployment(unittest.TestCase):
    def test_docker_only_build_and_failure_propagation(self):
        with tempfile.TemporaryDirectory(prefix='chublog deploy ') as temporary:
            root = Path(temporary)
            (root / 'scripts').mkdir()
            shutil.copy2(SCRIPT, root / 'scripts/prepare-deploy.py')
            for name in ['external/ChuSQL-RustDriver/Cargo.toml',
                         'external/ChuSQL/chusql-core/storage/Cargo.toml']:
                manifest = root / name
                manifest.parent.mkdir(parents=True)
                manifest.touch()
            (root / '.env.example').touch()
            tools = root / 'tools'
            tools.mkdir()
            docker = tools / 'docker'
            docker.write_text(f'#!{sys.executable}\nimport json, os, sys\n'
                              'from pathlib import Path\n'
                              'Path(os.environ["CALL_LOG"]).write_text(json.dumps(sys.argv[1:]))\n'
                              'sys.exit(int(os.environ.get("DOCKER_EXIT", "0")))\n')
            docker.chmod(0o755)
            log = root / 'call.json'
            env = {**os.environ, 'PATH': str(tools), 'CALL_LOG': str(log)}
            command = [sys.executable, str(root / 'scripts/prepare-deploy.py')]
            result = subprocess.run(command + ['--build-database'], env=env, capture_output=True, text=True)
            self.assertEqual(result.returncode, 0, result.stderr)
            call = json.loads(log.read_text())
            self.assertEqual(call[:3], ['compose', '--env-file', str(root / '.env.example')])
            self.assertIn('db', call)
            self.assertIn('web', call)
            self.assertFalse((root / 'deploy/bin').exists())
            override = root / 'public sources.env'
            override.write_text('REGISTRY_PREFIX=public.ecr.aws/docker/library/\n')
            result = subprocess.run(command + ['--sources-env-file', str(override)],
                                    env=env, capture_output=True, text=True)
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertEqual(json.loads(log.read_text())[3:5], ['--env-file', str(override)])
            (root / '.env').touch()
            result = subprocess.run(command, env={**env, 'DOCKER_EXIT': '17'}, capture_output=True, text=True)
            self.assertEqual(result.returncode, 17)
            self.assertEqual(json.loads(log.read_text())[2], str(root / '.env'))
            docker.unlink()
            result = subprocess.run(command, env=env, capture_output=True, text=True)
            self.assertEqual(result.returncode, 2)
            self.assertIn('Docker with the Compose plugin is required', result.stderr)


if __name__ == '__main__':
    unittest.main()

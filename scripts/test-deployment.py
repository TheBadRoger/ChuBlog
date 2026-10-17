#!/usr/bin/env python3
"""Check unattended deployment, safe updates, and scoped uninstall without Docker."""
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]


class Deployment(unittest.TestCase):
    def test_deploy_update_and_uninstall_lifecycle(self):
        with tempfile.TemporaryDirectory(prefix='chublog lifecycle ') as temporary:
            root = Path(temporary)
            (root / 'scripts').mkdir()
            for name in ['deployment.py', 'prepare-deploy.py', 'deploy.sh', 'update.sh', 'uninstall.sh']:
                shutil.copy2(ROOT / 'scripts' / name, root / 'scripts' / name)
            shutil.copy2(ROOT / '.env.example', root / '.env.example')
            for name in ['external/ChuSQL-RustDriver/Cargo.toml', 'external/ChuSQL/chusql-core/storage/Cargo.toml']:
                manifest = root / name
                manifest.parent.mkdir(parents=True)
                manifest.touch()
            (root / 'scripts/fetch-deps.py').write_text(
                'import os\nfrom pathlib import Path\n'
                'with Path(os.environ["CALL_LOG"]).open("a") as log: log.write(\'["fetch"]\\n\')\n')
            tools = root / 'tools'
            tools.mkdir()
            program = f'''#!{sys.executable}
import json, os, sys
from pathlib import Path
kind = Path(sys.argv[0]).name
args = sys.argv[1:]
with Path(os.environ['CALL_LOG']).open('a') as log:
    log.write(json.dumps([kind, *args]) + '\\n')
if kind == 'git' and args[:2] == ['status', '--porcelain']:
    print(os.environ.get('GIT_DIRTY', ''), end='')
'''
            for name in ['docker', 'git']:
                tool = tools / name
                tool.write_text(program)
                tool.chmod(0o755)
            log = root / 'calls.jsonl'
            env = {key: value for key, value in os.environ.items()
                   if key not in ['CHUSQL_ROOT_PASSWORD', 'CHUSQL_PASSWORD', 'WEB_PORT', 'SSH_PORT']}
            env.update(PATH=str(tools) + os.pathsep + env['PATH'], CALL_LOG=str(log))

            def invoke(name, *args, extra_env=None):
                return subprocess.run([str(root / 'scripts' / name), *args], cwd=root,
                                      env={**env, **(extra_env or {})}, stdin=subprocess.DEVNULL,
                                      capture_output=True, text=True)

            result = invoke('deploy.sh')
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
            settings = (root / '.env').read_text()
            self.assertNotIn('ADMIN_PASSWORD_HASH', settings)
            self.assertNotIn('PUBLIC_ORIGIN=', settings)
            self.assertNotIn('SITE_ADDRESS=', settings)
            self.assertIn('WEB_PORT=1741', settings)
            self.assertIn('SSH_PORT=2222', settings)
            self.assertEqual((root / '.env').stat().st_mode & 0o777, 0o600)
            passwords = [line.split('=', 1)[1] for line in settings.splitlines()
                         if line.startswith(('CHUSQL_ROOT_PASSWORD=', 'CHUSQL_PASSWORD='))]
            self.assertTrue(all(len(value) >= 16 for value in passwords))
            self.assertNotEqual(*passwords)
            self.assertTrue(all(value not in result.stdout + result.stderr for value in passwords))
            calls = [json.loads(line) for line in log.read_text().splitlines()]
            self.assertEqual(calls[0], ['fetch'])
            actions = [call for call in calls if call[0] == 'docker']
            self.assertIn('build', actions[0])
            self.assertTrue({'api', 'ssh', 'db', 'web'}.issubset(actions[0]))
            self.assertEqual(actions[1][-4:], ['up', '-d', '--no-build', 'db'])
            self.assertIn('provision-db', actions[2])
            self.assertIn('--wait', actions[3])
            self.assertFalse(any('hash-password' in call for call in calls))
            result = invoke('deploy.sh')
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertEqual((root / '.env').read_text(), settings)
            (root / '.env').write_text(settings + 'ADMIN_USERNAME=old-admin\nADMIN_PASSWORD_HASH=old-hash\nPUBLIC_ORIGIN=https://old.example\nSITE_ADDRESS=old.example\nHTTP_PORT=80\nHTTPS_PORT=443\nALLOW_REGISTRATION=false\n')
            result = invoke('deploy.sh')
            self.assertEqual(result.returncode, 0, result.stderr)
            settings = (root / '.env').read_text()
            self.assertNotIn('ADMIN_USERNAME=', settings)
            self.assertNotIn('ADMIN_PASSWORD_HASH=', settings)
            self.assertIn('ALLOW_REGISTRATION=false', settings)
            self.assertNotIn('PUBLIC_ORIGIN=', settings)
            self.assertNotIn('HTTP_PORT=', settings)
            self.assertTrue(all(password in settings for password in passwords))
            log.write_text('')
            result = invoke('update.sh', extra_env={'GIT_DIRTY': ' M source.rs\n'})
            self.assertNotEqual(result.returncode, 0)
            self.assertNotIn('pull', log.read_text())
            self.assertNotIn('docker', log.read_text())
            log.write_text('')
            result = invoke('update.sh')
            self.assertEqual(result.returncode, 0, result.stderr)
            calls = [json.loads(line) for line in log.read_text().splitlines()]
            self.assertEqual(calls[:3], [['git', 'status', '--porcelain'], ['git', 'pull', '--ff-only'], ['fetch']])
            for bad_port in ['0', '65536', 'not-a-port']:
                result = invoke('deploy.sh', extra_env={'WEB_PORT': bad_port})
                self.assertNotEqual(result.returncode, 0)
                self.assertEqual((root / '.env').read_text(), settings)
            result = invoke('deploy.sh', extra_env={'WEB_PORT': '01741', 'SSH_PORT': '1741'})
            self.assertNotEqual(result.returncode, 0)
            self.assertEqual((root / '.env').read_text(), settings)
            result = invoke('deploy.sh', extra_env={'WEB_PORT': '18081', 'SSH_PORT': '12223'})
            self.assertEqual(result.returncode, 0, result.stderr)
            settings = (root / '.env').read_text()
            self.assertIn('WEB_PORT=18081', settings)
            self.assertIn('SSH_PORT=12223', settings)
            for purge in [False, True]:
                log.write_text('')
                result = invoke('uninstall.sh', *(['--purge'] if purge else []))
                self.assertEqual(result.returncode, 0, result.stderr)
                calls = [json.loads(line) for line in log.read_text().splitlines()]
                self.assertEqual(len(calls), 1)
                self.assertIn('down', calls[0])
                self.assertEqual('--volumes' in calls[0], purge)
                self.assertNotIn('prune', calls[0])
                self.assertEqual((root / '.env').read_text(), settings)
            (root / '.env').unlink()
            result = invoke('uninstall.sh', '--purge')
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertFalse((root / '.env').exists())
            bad_sources = root / 'bad.env'
            bad_sources.write_text('CHUSQL_ROOT_PASSWORD=not-a-download-source\n')
            result = invoke('deploy.sh', '--sources-env-file', str(bad_sources))
            self.assertEqual(result.returncode, 2)
            self.assertFalse((root / '.env').exists())


if __name__ == '__main__':
    unittest.main()

#!/usr/bin/env python3
"""Check remote installer fallback order without network or system writes."""
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]


class RemoteInstaller(unittest.TestCase):
    def test_each_site_is_exhausted_before_next_and_success_stops(self):
        for succeeds in ['repo.chunana.cn', 'github.com', 'never']:
            with self.subTest(succeeds=succeeds), tempfile.TemporaryDirectory() as temporary:
                root = Path(temporary)
                for directory in ['tools', 'tmp', 'etc/chusql', 'var/lib', 'var/log', 'usr/lib']:
                    (root / directory).mkdir(parents=True, exist_ok=True)
                script = (ROOT / 'deploy/install-chusql-remote.sh').read_text()
                for prefix in ['/tmp/', '/etc/', '/var/', '/usr/lib/']:
                    script = script.replace(prefix, str(root) + prefix)
                (root / 'install.sh').write_text(script)
                curl = root / 'tools/curl'
                curl.write_text(f'''#!{sys.executable}
import json, os, sys
from pathlib import Path
args = sys.argv[1:]
url = args[args.index('-o') - 1]
out = Path(args[args.index('-o') + 1])
with Path(os.environ['LOG']).open('a') as log:
    log.write(url + '\\n')
if url.endswith('/latest'):
    out.write_text(json.dumps({{'tag_name': 'v1', 'assets': [{{'name': 'chusql-linux.tar.gz'}}]}}))
else:
    out.write_text('case \" $* \" in *\" --low-performance-adapted \"*) ;; *) exit 99 ;; esac\\nexit ' + ('0' if os.environ['SUCCEEDS'].replace('github.com', 'raw.githubusercontent.com') in url else '1') + '\\n')
''')
                curl.chmod(0o755)
                log = root / 'calls'
                result = subprocess.run(['sh', str(root / 'install.sh')], capture_output=True, text=True,
                                        env={**os.environ, 'PATH': str(root / 'tools') + ':' + os.environ['PATH'],
                                             'LOG': str(log), 'SUCCEEDS': succeeds})
                calls = log.read_text().splitlines()
                self.assertEqual(result.returncode, 1 if succeeds == 'never' else 0, result.stderr)
                self.assertIn('127.0.0.1:3000', calls[0])
                self.assertIn('repo.chunana.cn', calls[2])
                self.assertEqual(len(calls), 4 if succeeds == 'repo.chunana.cn' else 6)
                if len(calls) == 6:
                    self.assertIn('raw.githubusercontent.com', calls[4])
                    self.assertIn('api.github.com', calls[5])


if __name__ == '__main__':
    unittest.main()

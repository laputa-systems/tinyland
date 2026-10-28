#!/usr/bin/env python3
import hashlib
import pathlib
import shutil
import subprocess
import tempfile
import unittest

ROOT = pathlib.Path(__file__).resolve().parent.parent


class GuestPackageLockTests(unittest.TestCase):
    def test_corrupt_cached_apk_is_rejected_before_package_installation(self):
        entries = [line.split() for line in (ROOT / 'guest/packages.sha256').read_text().splitlines()]
        with tempfile.TemporaryDirectory(dir=ROOT / '.work/tmp') as directory:
            fixture = pathlib.Path(directory)
            (fixture / 'harness').mkdir()
            (fixture / 'guest').mkdir()
            shutil.copyfile(ROOT / 'harness/prepare-guest.sh', fixture / 'harness/prepare-guest.sh')
            shutil.copyfile(ROOT / 'guest/packages.sha256', fixture / 'guest/packages.sha256')
            cache = fixture / '.work/dl'
            cache.mkdir(parents=True)
            tarball = 'alpine-minirootfs-3.20.3-x86_64.tar.gz'
            (cache / tarball).hardlink_to(ROOT / '.work/dl' / tarball)
            for digest, name in entries:
                source = ROOT / '.work/dl/apk' / name
                target = cache / 'apk' / name
                target.parent.mkdir(parents=True, exist_ok=True)
                target.hardlink_to(source)
            digest, name = entries[0]
            corrupt = cache / 'apk' / name
            original = corrupt.read_bytes()
            corrupt.unlink()
            corrupt.write_bytes(original[:-1] + bytes([original[-1] ^ 1]))
            result = subprocess.run(['bash', 'harness/prepare-guest.sh', str(fixture / '.work/guest-root')],
                                    cwd=fixture, capture_output=True, text=True, timeout=30)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn(f'{name}: FAILED', result.stdout)
            self.assertNotIn('GUEST_PACKAGES_VERIFIED', result.stdout)
            self.assertEqual(hashlib.sha256((ROOT / '.work/dl/apk' / name).read_bytes()).hexdigest(), digest)


if __name__ == '__main__':
    unittest.main()

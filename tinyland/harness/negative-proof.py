#!/usr/bin/env python3
import pathlib
import struct
import subprocess
import zlib

ROOT = pathlib.Path(__file__).resolve().parent.parent
SCRATCH = ROOT / '.work' / 'tmp' / 'negative-proof'


def png(path, color):
    def chunk(kind, data):
        return struct.pack('>I', len(data)) + kind + data + struct.pack('>I', zlib.crc32(kind + data))
    path.write_bytes(b'\x89PNG\r\n\x1a\n' + chunk(b'IHDR', struct.pack('>IIBBBBB', 8, 8, 8, 2, 0, 0, 0))
                     + chunk(b'IDAT', zlib.compress((b'\0' + bytes(color) * 8) * 8)) + chunk(b'IEND', b''))


def check(console, shot, expected=None):
    result = subprocess.run(['bash', 'harness/check-proof.sh', 'smoke', str(console), str(shot)],
                            cwd=ROOT, capture_output=True, text=True, check=False)
    output = result.stdout + result.stderr
    if expected is None:
        assert result.returncode == 0, output
    else:
        assert result.returncode != 0 and expected in output, output


def main():
    SCRATCH.mkdir(parents=True, exist_ok=True)
    console = ROOT / '.work' / 'smoke-console.log'
    shot = ROOT / '.work' / 'smoke.png'
    check(console, shot)
    text = console.read_text()
    missing_input = SCRATCH / 'no-input.log'
    missing_input.write_text('\n'.join(line for line in text.splitlines() if 'GUEST_TEXT_OK' not in line))
    check(missing_input, shot, 'input not confirmed')
    black, white = SCRATCH / 'blank.png', SCRATCH / 'missing-marker.png'
    png(black, (0, 0, 0))
    png(white, (255, 255, 255))
    check(console, black, 'FAIL: nonblack')
    check(console, white, 'missing magenta marker')
    crash = SCRATCH / 'early-crash.log'
    crash.write_text(text + '\nTINYLAND_EXIT rc=1\n')
    check(crash, shot, 'guest or compositor exited')
    print('NEGATIVE_PROOF_OK: no input, blank framebuffer, missing marker, early crash')


if __name__ == '__main__':
    main()

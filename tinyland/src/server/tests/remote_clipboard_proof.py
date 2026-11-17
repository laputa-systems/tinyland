import os
import selectors
import struct
import subprocess
import tempfile
import time

with tempfile.TemporaryDirectory(dir=os.environ['TMPDIR']) as root:
    os.chmod(root, 0o700)
    env = dict(os.environ, XDG_RUNTIME_DIR=root, REMOTE_MARKER=root + '/marker')
    process = subprocess.Popen([
        os.environ['TINYLAND_BIN'], '--remote', '--width', '640', '--height', '480',
        '--socket', 'wayland-proof', '--', 'foot', '--', '/bin/sh', '-c',
        'printf "READY\\n"; read line; [ "$line" = "pasted λ" ] && echo received > "$REMOTE_MARKER"; sleep 60',
    ], env=env, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    selector = selectors.DefaultSelector()
    selector.register(process.stdout, selectors.EVENT_READ)

    def readn(count):
        result = b''
        while len(result) < count:
            if not selector.select(15):
                raise RuntimeError('timed out receiving compositor message')
            part = os.read(process.stdout.fileno(), count - len(result))
            if not part:
                raise RuntimeError('compositor exited')
            result += part
        return result

    def message():
        header = readn(9)
        assert header[:4] == b'TLR1'
        return header[4], readn(struct.unpack('<I', header[5:])[0])

    def send(tag, payload):
        process.stdin.write(b'TLR1' + bytes([tag]) + struct.pack('<I', len(payload)) + payload)
        process.stdin.flush()

    def key(code, down):
        send(17, struct.pack('<IHB', 1, code, down))

    try:
        tag, body = message()
        assert tag == 1 and struct.unpack('<II', body) == (640, 480)
        while True:
            tag, body = message()
            assert tag == 2 and len(body) == 8 + 640 * 480 * 4
            send(16, body[:8])
            if any(body[8:]):
                break
        send(22, struct.pack('<I', 1) + 'pasted λ'.encode())
        for code, down in [(29, 1), (42, 1), (47, 1), (47, 0), (42, 0), (29, 0), (28, 1), (28, 0)]:
            key(code, down)
        deadline = time.monotonic() + 15
        while not os.path.exists(root + '/marker') and time.monotonic() < deadline:
            if selector.select(0.1):
                tag, body = message()
                if tag == 2:
                    send(16, body[:8])
        assert os.path.exists(root + '/marker'), 'terminal did not receive clipboard paste'
        print('real foot: remote clipboard pasted via Ctrl+Shift+V')
    finally:
        process.stdin.close()
        try:
            process.wait(timeout=10)
        except subprocess.TimeoutExpired:
            process.kill()
            process.wait()
        print('compositor exit:', process.returncode)
        print(process.stderr.read().decode(errors='replace')[-4000:])

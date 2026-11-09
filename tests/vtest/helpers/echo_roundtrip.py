#!/usr/bin/env python3
"""Send DATA to 127.0.0.1:PORT and expect it echoed back.
Usage: echo_roundtrip.py PORT DATA"""
import socket
import sys

port, data = int(sys.argv[1]), sys.argv[2].encode()
s = socket.create_connection(("127.0.0.1", port))
s.settimeout(5)
s.sendall(data)
got = b""
while len(got) < len(data):
    d = s.recv(4096)
    if not d:
        break
    got += d
if got != data:
    sys.exit(f"FAIL: sent {data!r}, got {got!r}")
print(f"OK: echo {data!r}")

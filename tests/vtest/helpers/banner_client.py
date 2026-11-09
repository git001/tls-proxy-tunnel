#!/usr/bin/env python3
"""Connect to 127.0.0.1:PORT, send nothing, expect BANNER (sent by the target
right after the proxy's 200), then check the relay echoes.
Usage: banner_client.py PORT BANNER"""
import socket
import sys

port, want = int(sys.argv[1]), sys.argv[2].encode() + b"\r\n"
s = socket.create_connection(("127.0.0.1", port))
s.settimeout(5)
got = b""
while len(got) < len(want):
    d = s.recv(100)
    if not d:
        break
    got += d
if got != want:
    sys.exit(f"FAIL: banner {got!r}, want {want!r}")
s.sendall(b"after")
got = s.recv(100)
if got != b"after":
    sys.exit(f"FAIL: relay after banner {got!r}")
print("OK: banner and relay")

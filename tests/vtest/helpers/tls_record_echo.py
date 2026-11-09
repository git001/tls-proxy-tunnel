#!/usr/bin/env python3
"""Send a minimal TLS handshake record (no SNI) to 127.0.0.1:PORT and expect
it echoed back. Usage: tls_record_echo.py PORT"""
import socket
import sys

hello = bytes([0x16, 0x03, 0x01, 0x00, 0x04, 1, 2, 3, 4])
s = socket.create_connection(("127.0.0.1", int(sys.argv[1])))
s.settimeout(5)
s.sendall(hello)
got = b""
while len(got) < len(hello):
    d = s.recv(100)
    if not d:
        break
    got += d
if got != hello:
    sys.exit(f"FAIL: echo {got!r}")
print("OK: TLS record echoed")

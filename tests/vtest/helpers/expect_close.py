#!/usr/bin/env python3
"""Connect to 127.0.0.1:PORT, send nothing, and expect the server to close
the connection after MIN..MAX seconds. Usage: expect_close.py PORT MIN MAX"""
import socket
import sys
import time

port, lo, hi = int(sys.argv[1]), float(sys.argv[2]), float(sys.argv[3])
s = socket.create_connection(("127.0.0.1", port))
s.settimeout(hi + 5)
t0 = time.monotonic()
data = s.recv(1)
took = time.monotonic() - t0
if data != b"" or not lo <= took <= hi:
    sys.exit(f"FAIL: got {data!r} after {took:.2f}s, want close after {lo}..{hi}s")
print(f"OK: closed after {took:.2f}s")

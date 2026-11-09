#!/usr/bin/env python3
"""Connect to 127.0.0.1:PORT, send DATA, and expect the server to close the
connection without sending anything (EOF or RST both count as closed).
Usage: send_expect_close.py PORT DATA"""
import socket
import sys

port, data = int(sys.argv[1]), sys.argv[2].encode()
s = socket.create_connection(("127.0.0.1", port))
s.settimeout(5)
s.sendall(data)
try:
    got = s.recv(4096)
except ConnectionResetError:
    got = b""  # a reset is an acceptable close as well
if got:
    sys.exit(f"FAIL: expected the connection to close, got {got!r}")
s.close()
print("OK: connection closed")

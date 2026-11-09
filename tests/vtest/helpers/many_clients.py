#!/usr/bin/env python3
"""Open COUNT connections to 127.0.0.1:PORT at once, send a byte on each,
hold them for a second, close them all. Usage: many_clients.py PORT COUNT"""
import socket
import sys
import time

port, count = int(sys.argv[1]), int(sys.argv[2])
socks = []
for _ in range(count):
    s = socket.create_connection(("127.0.0.1", port))
    s.sendall(b"x")
    socks.append(s)
time.sleep(1)
for s in socks:
    s.close()
time.sleep(1)
print(f"OK: {count} clients opened and closed")

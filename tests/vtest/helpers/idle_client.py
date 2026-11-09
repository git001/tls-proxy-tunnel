#!/usr/bin/env python3
"""Through an echo relay with an idle timeout of IDLE seconds on
127.0.0.1:PORT: send every 0.4s for ACTIVE seconds (must stay open), then
stay silent (must be closed about IDLE seconds after the last data).
Usage: idle_client.py PORT ACTIVE IDLE"""
import socket
import sys
import time

port, active, idle = int(sys.argv[1]), float(sys.argv[2]), float(sys.argv[3])
s = socket.create_connection(("127.0.0.1", port))
s.settimeout(idle + 5)
t_end = time.monotonic() + active
while True:
    s.sendall(b"ping")
    if s.recv(100) != b"ping":
        sys.exit("FAIL: closed while active")
    last = time.monotonic()  # the idle clock starts here
    if last >= t_end:
        break
    time.sleep(0.4)
print(f"OK: active relay stayed open for {active}s")
data = s.recv(1)
took = time.monotonic() - last
if data != b"" or not 0.8 * idle <= took <= idle + 4:
    sys.exit(f"FAIL: idle relay: {data!r} after {took:.2f}s, want close after ~{idle}s")
print(f"OK: idle relay closed {took:.2f}s after the last data")

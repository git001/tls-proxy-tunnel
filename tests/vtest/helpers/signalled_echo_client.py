#!/usr/bin/env python3
"""Connect to 127.0.0.1:PORT, send DATA, touch READY_FILE (so the test can
act while the relay is in flight, e.g. send SIGTERM to tpt), then wait for
DATA echoed back.
Usage: signalled_echo_client.py PORT DATA READY_FILE"""
import socket
import sys

port, data, ready = int(sys.argv[1]), sys.argv[2].encode(), sys.argv[3]
s = socket.create_connection(("127.0.0.1", port))
s.settimeout(5)
s.sendall(data)
open(ready, "w").close()
got = b""
while len(got) < len(data):
    d = s.recv(4096)
    if not d:
        sys.exit("FAIL: connection closed before the echo arrived")
    got += d
if got != data:
    sys.exit(f"FAIL: echo mismatch: got {got!r}")
s.shutdown(socket.SHUT_WR)
s.close()
print(f"OK: echo {data!r} received")

#!/usr/bin/env python3
"""Send several payloads, one after the other, over one connection to
127.0.0.1:PORT and expect each one echoed back before the next is sent.
A payload "hex:0001ff" is sent as raw bytes, anything else as text.
--pause waits that many seconds after each echo (keeps the relay alive).
Usage: echo_sequence.py PORT [--pause SECONDS] PAYLOAD..."""
import argparse
import socket
import sys
import time

ap = argparse.ArgumentParser()
ap.add_argument("port", type=int)
ap.add_argument("--pause", type=float, default=0.0)
ap.add_argument("payloads", nargs="+")
args = ap.parse_args()

s = socket.create_connection(("127.0.0.1", args.port))
s.settimeout(5)
for p in args.payloads:
    data = bytes.fromhex(p[4:]) if p.startswith("hex:") else p.encode()
    s.sendall(data)
    got = b""
    while len(got) < len(data):
        d = s.recv(len(data) - len(got))
        if not d:
            sys.exit(f"FAIL: connection closed after {len(got)} of {len(data)} bytes")
        got += d
    if got != data:
        sys.exit(f"FAIL: sent {data!r}, got {got!r}")
    if args.pause:
        time.sleep(args.pause)
s.shutdown(socket.SHUT_WR)
s.close()
print(f"OK: {len(args.payloads)} payloads echoed")

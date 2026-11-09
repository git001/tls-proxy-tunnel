#!/usr/bin/env python3
"""Connect to 127.0.0.1:PORT, send DATA, optionally expect it echoed back,
optionally touch READY_FILE, then keep the connection open for SECONDS and
close it. Used to occupy a maxclients slot.
Usage: occupy_slot.py PORT DATA SECONDS [--expect-echo] [--ready FILE]"""
import argparse
import socket
import sys
import time

ap = argparse.ArgumentParser()
ap.add_argument("port", type=int)
ap.add_argument("data")
ap.add_argument("seconds", type=float)
ap.add_argument("--expect-echo", action="store_true")
ap.add_argument("--ready")
args = ap.parse_args()

data = args.data.encode()
s = socket.create_connection(("127.0.0.1", args.port))
s.settimeout(5)
s.sendall(data)
if args.expect_echo:
    got = b""
    while len(got) < len(data):
        d = s.recv(len(data) - len(got))
        if not d:
            break
        got += d
    if got != data:
        sys.exit(f"FAIL: echo expected {data!r}, got {got!r}")
if args.ready:
    open(args.ready, "w").close()
time.sleep(args.seconds)
s.close()
print(f"OK: held the connection for {args.seconds}s")

#!/usr/bin/env python3
"""Direct TCP backend on 127.0.0.1:PORT that must receive raw TLS, not an
HTTP CONNECT request. The first bytes of each connection must start with a
TLS record header (0x16 0x03); then everything is echoed back. On a CONNECT
request or any other data it prints "FAIL: ..." and exits with status 1.
Prints "OK: received raw TLS bytes (no CONNECT prefix)" per good connection.
Usage: raw_tls_backend.py PORT"""
import os
import socket
import sys
import threading


def fail(conn, msg):
    print(f"FAIL: {msg}", file=sys.stderr, flush=True)
    conn.close()
    os._exit(1)


def handle(conn):
    with conn:
        first = conn.recv(8)
        if not first:
            return
        if first[:8] == b"CONNECT ":
            fail(conn, "received a CONNECT request, direct TCP override did not apply")
        if first[:2] != b"\x16\x03":
            fail(conn, f"expected a TLS record (0x16 0x03), got {first[:4]!r}")
        print("OK: received raw TLS bytes (no CONNECT prefix)", flush=True)
        conn.sendall(first)
        while True:
            d = conn.recv(4096)
            if not d:
                return
            conn.sendall(d)


srv = socket.socket()
srv.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
srv.bind(("127.0.0.1", int(sys.argv[1])))
srv.listen(16)
while True:
    c, _ = srv.accept()
    threading.Thread(target=handle, args=(c,), daemon=True).start()
    del c

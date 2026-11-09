#!/usr/bin/env python3
"""HTTP CONNECT proxy backend on 127.0.0.1:PORT for tpt's `via:` tests.

Every connection: read the CONNECT request up to the blank line, run the
optional checks, then answer with the next status of --status (the list is
used round-robin over successive connections). A 200 is followed by the
--after behaviour; any other status closes the connection after the answer.

If a check fails, the backend prints "FAIL: ..." to stderr, answers 502,
and exits with status 1 (so tpt closes the client and the test fails).

Usage: connect_backend.py PORT [--status 200[,403,...]]
           [--expect-target HOST:PORT] [--expect-header "Name: value"]
           [--expect-numbered-headers PREFIX:COUNT] [--min-request-size N]
           [--recv-size N] [--fragment] [--after echo|drain|once]
           [--echo-delay SECONDS]

  --expect-target            the request line must be "CONNECT HOST:PORT ..."
  --expect-header            the header Name must be present with exactly value
  --expect-numbered-headers  headers PREFIX01 .. PREFIXnn must all be present
  --min-request-size         the request must be longer than N bytes
  --recv-size                read the request in chunks of N bytes
  --fragment                 send the 200 response in three TCP segments
  --after echo               echo everything until the client closes (default)
  --after drain              read and discard everything until close
  --after once               read once, wait --echo-delay seconds, echo, close
"""
import argparse
import os
import socket
import sys
import threading
import time

REASONS = {
    200: "Connection Established",
    403: "Forbidden",
    407: "Proxy Authentication Required",
    418: "I'm a teapot",
    502: "Bad Gateway",
    503: "Service Unavailable",
}
EXTRA_HEADERS = {
    407: 'Proxy-Authenticate: Basic realm="proxy"\r\n',
}

ap = argparse.ArgumentParser()
ap.add_argument("port", type=int)
ap.add_argument("--status", default="200")
ap.add_argument("--expect-target")
ap.add_argument("--expect-header")
ap.add_argument("--expect-numbered-headers")
ap.add_argument("--min-request-size", type=int, default=0)
ap.add_argument("--recv-size", type=int, default=4096)
ap.add_argument("--fragment", action="store_true")
ap.add_argument("--after", choices=["echo", "drain", "once"], default="echo")
ap.add_argument("--echo-delay", type=float, default=0.0)
args = ap.parse_args()

statuses = [int(s) for s in args.status.split(",")]
counter = 0
lock = threading.Lock()


def response(code):
    return (f"HTTP/1.1 {code} {REASONS[code]}\r\n"
            f"{EXTRA_HEADERS.get(code, '')}\r\n").encode()


def fail(conn, msg):
    print(f"FAIL: {msg}", file=sys.stderr, flush=True)
    conn.sendall(response(502))
    conn.close()
    os._exit(1)


def check(conn, req):
    lines = req.decode("utf-8", errors="replace").split("\r\n")
    headers = {}
    for line in lines[1:]:
        if ":" in line:
            name, value = line.split(":", 1)
            headers[name.strip().lower()] = value.strip()

    if args.expect_target:
        parts = lines[0].split(" ")
        if len(parts) < 2 or parts[1] != args.expect_target:
            fail(conn, f"expected CONNECT {args.expect_target}, got {lines[0]!r}")
        print(f"OK: received CONNECT {parts[1]}", flush=True)

    if args.expect_header:
        name, want = (x.strip() for x in args.expect_header.split(":", 1))
        got = headers.get(name.lower())
        if got != want:
            fail(conn, f"header {name}: expected {want!r}, got {got!r}")
        print(f"OK: header {name} correct: {got}", flush=True)

    if args.min_request_size and len(req) <= args.min_request_size:
        fail(conn, f"CONNECT request too small ({len(req)} bytes, "
                   f"expected > {args.min_request_size})")

    if args.expect_numbered_headers:
        prefix, count = args.expect_numbered_headers.rsplit(":", 1)
        names = [f"{prefix}{i:02d}" for i in range(1, int(count) + 1)]
        missing = [n for n in names if n.lower() not in headers]
        if missing:
            fail(conn, f"missing headers: {missing}")
        print(f"OK: CONNECT request complete ({len(req)} bytes, "
              f"all {count} headers present)", flush=True)


def send_ok(conn):
    if not args.fragment:
        conn.sendall(response(200))
        return
    # One send() per TCP segment, so tpt has to accumulate the response.
    conn.setsockopt(socket.IPPROTO_TCP, socket.TCP_NODELAY, 1)
    for i, part in enumerate([b"HTTP/1.1 200 ", b"Connection Established\r\n", b"\r\n"]):
        if i:
            time.sleep(0.03)
        conn.sendall(part)


def handle(conn):
    global counter
    with conn:
        req = b""
        while b"\r\n\r\n" not in req:
            chunk = conn.recv(args.recv_size)
            if not chunk:
                return
            req += chunk
        check(conn, req)

        with lock:
            code = statuses[counter % len(statuses)]
            counter += 1
        if code != 200:
            conn.sendall(response(code))
            return

        send_ok(conn)
        if args.after == "once":
            data = conn.recv(4096)
            if data:
                time.sleep(args.echo_delay)
                conn.sendall(data)
            return
        while True:
            data = conn.recv(4096)
            if not data:
                return
            if args.after == "echo":
                conn.sendall(data)


srv = socket.socket()
srv.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
srv.bind(("127.0.0.1", args.port))
srv.listen(16)
while True:
    c, _ = srv.accept()
    threading.Thread(target=handle, args=(c,), daemon=True).start()
    del c  # the thread owns the socket; closing is up to it

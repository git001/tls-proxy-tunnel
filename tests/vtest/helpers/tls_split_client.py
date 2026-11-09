#!/usr/bin/env python3
"""Send a real TLS ClientHello for SNI to 127.0.0.1:PORT in several pieces
and expect it echoed back (the SNI routes to echo, the default is ban).
Case 1: a normal ClientHello in 3 pieces. Case 2: one padded past 4 KiB
(long ALPN list) in 4 pieces. Usage: tls_split_client.py PORT SNI"""
import socket
import ssl
import sys
import time

PORT, SNI = int(sys.argv[1]), sys.argv[2]


def client_hello(alpn=None):
    ctx = ssl.create_default_context()
    if alpn:
        ctx.set_alpn_protocols(alpn)
    inc, out = ssl.MemoryBIO(), ssl.MemoryBIO()
    obj = ctx.wrap_bio(inc, out, server_hostname=SNI)
    try:
        obj.do_handshake()
    except ssl.SSLWantReadError:
        pass
    return out.read()


def check(name, hello, parts):
    s = socket.create_connection(("127.0.0.1", PORT))
    s.settimeout(5)
    step = len(hello) // parts + 1
    for i in range(0, len(hello), step):
        s.sendall(hello[i:i + step])
        time.sleep(0.05)
    got = b""
    while len(got) < len(hello):
        d = s.recv(65536)
        if not d:
            break
        got += d
    s.close()
    if got != hello:
        sys.exit(f"FAIL {name}: {len(hello)} bytes in {parts} parts, "
                 f"{len(got)} echoed (SNI not found, banned)")
    print(f"OK {name}: {len(hello)} bytes in {parts} parts")


check("split", client_hello(), 3)
big = client_hello(["p%03d-" % i + "x" * 200 for i in range(30)])
assert len(big) > 4096, len(big)
check("large", big, 4)

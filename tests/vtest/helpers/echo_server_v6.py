#!/usr/bin/env python3
"""TCP echo server on [::1]:PORT, IPv6 only (IPV6_V6ONLY), so a client that
reaches it must have used IPv6. Usage: echo_server_v6.py PORT"""
import socket
import sys
import threading


def handle(conn):
    with conn:
        while True:
            d = conn.recv(4096)
            if not d:
                return
            conn.sendall(d)


srv = socket.socket(socket.AF_INET6, socket.SOCK_STREAM)
srv.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
srv.setsockopt(socket.IPPROTO_IPV6, socket.IPV6_V6ONLY, 1)
srv.bind(("::1", int(sys.argv[1])))
srv.listen(16)
while True:
    c, _ = srv.accept()
    threading.Thread(target=handle, args=(c,), daemon=True).start()
    del c

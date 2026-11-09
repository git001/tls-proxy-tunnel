#!/usr/bin/env python3
"""TCP echo server on 127.0.0.1:PORT. Usage: echo_server.py PORT"""
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


srv = socket.socket()
srv.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
srv.bind(("127.0.0.1", int(sys.argv[1])))
srv.listen(16)
while True:
    c, _ = srv.accept()
    threading.Thread(target=handle, args=(c,), daemon=True).start()
    del c

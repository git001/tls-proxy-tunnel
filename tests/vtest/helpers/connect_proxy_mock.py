#!/usr/bin/env python3
"""HTTP CONNECT proxy mock on 127.0.0.1:PORT. Reads the CONNECT request, then
  MODE=silent  never answers (holds the connection)
  MODE=banner  sends "200" and a target banner in one send, then echoes
Usage: connect_proxy_mock.py PORT MODE"""
import socket
import sys
import threading
import time

PORT, MODE = int(sys.argv[1]), sys.argv[2]
BANNER = b"SSH-2.0-test\r\n"


def handle(conn):
    with conn:
        buf = b""
        while b"\r\n\r\n" not in buf:
            chunk = conn.recv(4096)
            if not chunk:
                return
            buf += chunk
        if MODE == "silent":
            time.sleep(30)
            return
        conn.sendall(b"HTTP/1.1 200 Connection established\r\n\r\n" + BANNER)
        while True:
            d = conn.recv(4096)
            if not d:
                return
            conn.sendall(d)


srv = socket.socket()
srv.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
srv.bind(("127.0.0.1", PORT))
srv.listen(16)
while True:
    c, _ = srv.accept()
    threading.Thread(target=handle, args=(c,), daemon=True).start()
    del c  # the thread owns the socket; closing is up to it

#!/usr/bin/env python3
"""Connect to 127.0.0.1:PORT, send one byte, then keep the connection open
for SECONDS without sending more. Usage: hold_connection.py PORT SECONDS"""
import socket
import sys
import time

s = socket.create_connection(("127.0.0.1", int(sys.argv[1])))
s.sendall(b"x")
time.sleep(float(sys.argv[2]))
s.close()

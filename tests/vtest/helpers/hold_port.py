#!/usr/bin/env python3
"""Listen on 127.0.0.1:PORT (without accepting) for SECONDS, so nobody else
can bind it. Usage: hold_port.py PORT SECONDS"""
import socket
import sys
import time

s = socket.socket()
s.bind(("127.0.0.1", int(sys.argv[1])))
s.listen(1)
time.sleep(float(sys.argv[2]))

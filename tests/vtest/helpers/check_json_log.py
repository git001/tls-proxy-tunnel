#!/usr/bin/env python3
"""Every non-empty line of LOGFILE is a JSON object with exactly
ts/level/target/msg, and one of them is the startup line."""
import json
import sys

lines = [l for l in open(sys.argv[1]) if l.strip()]
for line in lines:
    rec = json.loads(line)
    assert set(rec) == {"ts", "level", "target", "msg"}, rec
assert any("tls-proxy-tunnel v" in json.loads(l)["msg"] for l in lines), lines
print(f"OK: {len(lines)} JSON log lines")

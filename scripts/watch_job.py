#!/usr/bin/env python3
import json, sys, time, urllib.request

job = sys.argv[1]
url = f"http://127.0.0.1:8080/jobs/{job}"
last = None
while True:
    try:
        d = json.load(urllib.request.urlopen(url))
    except Exception as e:
        print("poll error:", e, flush=True)
        time.sleep(10)
        continue
    cur = (d.get("state"), d.get("stage"))
    if cur != last:
        print(f"{cur[0]} / {cur[1]}", flush=True)
        last = cur
    if d.get("state") in ("completed", "failed", "already_completed"):
        print("=== TERMINAL ===", flush=True)
        print(json.dumps(d, indent=2), flush=True)
        break
    time.sleep(12)

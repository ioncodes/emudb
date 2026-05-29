#!/usr/bin/env python3
import argparse
import concurrent.futures
import json
import os
import subprocess
import sys

try:
    import tomllib
except ModuleNotFoundError:
    import tomli as tomllib

SCRIPT_DIR = os.path.dirname(os.path.abspath(__file__))


def load_config():
    with open(os.path.join(SCRIPT_DIR, "screenshotter.toml"), "rb") as f:
        return tomllib.load(f)


def run_game(cfg, game):
    gecko = cfg.get("gecko", {})
    runner = cfg.get("runner", {})
    out_dir = game["output_dir"]
    os.makedirs(out_dir, exist_ok=True)

    env = dict(os.environ)
    env["GECKO_BIOS_DIR"] = gecko.get("bios_dir", "/config")

    cmd = [
        gecko.get("worker_binary", "/usr/local/bin/gecko-screenshotter-worker"),
        game["input"],
        out_dir,
    ]
    timeout = int(runner.get("timeout_seconds", 1200))

    print(f"[{game['id']}] $ {' '.join(cmd)}", flush=True)
    try:
        proc = subprocess.run(cmd, env=env, timeout=timeout, capture_output=True, text=True)
    except subprocess.TimeoutExpired:
        return game["id"], False, f"timeout after {timeout}s"

    log = (proc.stdout or "") + (proc.stderr or "")
    if proc.returncode != 0:
        return game["id"], False, f"worker exited {proc.returncode}\n{log}"

    pngs = [f for f in os.listdir(out_dir) if f.endswith(".png")]
    if not pngs:
        return game["id"], False, f"no frames produced\n{log}"

    return game["id"], True, f"{len(pngs)} frame(s)"


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--job", required=True)
    ap.add_argument("--input", required=True)
    ap.add_argument("--output", required=True)
    args = ap.parse_args()

    cfg = load_config()
    with open(args.job) as f:
        manifest = json.load(f)

    games = manifest.get("games", [])
    parallel = max(1, int(cfg.get("runner", {}).get("max_parallel_games", 1)))
    print(f"running {len(games)} game(s), up to {parallel} in parallel", flush=True)

    failures = 0
    with concurrent.futures.ThreadPoolExecutor(max_workers=parallel) as pool:
        futures = [pool.submit(run_game, cfg, g) for g in games]
        for fut in concurrent.futures.as_completed(futures):
            gid, ok, detail = fut.result()
            if ok:
                print(f"[{gid}] OK ({detail})", flush=True)
            else:
                failures += 1
                print(f"[{gid}] FAILED: {detail}", file=sys.stderr, flush=True)

    if failures:
        print(f"{failures}/{len(games)} game(s) failed", file=sys.stderr, flush=True)
        return 1
    print("all games succeeded", flush=True)
    return 0


if __name__ == "__main__":
    sys.exit(main())

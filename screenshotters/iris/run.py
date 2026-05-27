#!/usr/bin/env python3
import argparse
import glob
import json
import os
import shutil
import signal
import subprocess
import sys

try:
    import tomllib
except ModuleNotFoundError:
    import tomli as tomllib

SCRIPT_DIR = os.path.dirname(os.path.abspath(__file__))
PNG_MAGIC = b"\x89PNG\r\n\x1a\n"


def load_config():
    with open(os.path.join(SCRIPT_DIR, "screenshotter.toml"), "rb") as f:
        return tomllib.load(f)


def is_png(path):
    try:
        if os.path.getsize(path) == 0:
            return False
        with open(path, "rb") as f:
            return f.read(8) == PNG_MAGIC
    except OSError:
        return False


def run_game(cfg, env, game):
    iris = cfg["iris"]
    out_dir = game["output_dir"]
    snap_dir = os.path.join(out_dir, "snap")
    os.makedirs(snap_dir, exist_ok=True)

    run_seconds = int(iris.get("run_seconds", 20))
    grace_seconds = int(iris.get("grace_seconds", 30))

    cmd = [
        iris.get("binary", "/usr/local/bin/iris"),
        "--headless",
        "--snap-on-exit",
        "-b", iris["bios"],
        "-i", game["input"],
    ]
    print(f"[{game['id']}] $ {' '.join(cmd)} (run {run_seconds}s -> SIGINT)", flush=True)

    proc = subprocess.Popen(
        cmd, cwd=out_dir, env=env,
        stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True,
    )
    try:
        proc.wait(timeout=run_seconds)
    except subprocess.TimeoutExpired:
        proc.send_signal(signal.SIGINT)
        try:
            proc.wait(timeout=grace_seconds)
        except subprocess.TimeoutExpired:
            proc.terminate()
            try:
                proc.wait(timeout=10)
            except subprocess.TimeoutExpired:
                proc.kill()

    out = proc.stdout.read() if proc.stdout else ""
    tail = "\n".join(l for l in out.splitlines() if not l.startswith(("ee:", "iop:")))[-1500:]

    shots = [s for s in sorted(glob.glob(os.path.join(snap_dir, "*.png")), key=os.path.getmtime)
             if os.path.basename(s) != "0.png"]
    if not shots:
        return game["id"], False, f"no screenshot produced\n{tail}"

    final = os.path.join(out_dir, "0.png")
    shutil.move(shots[-1], final)
    shutil.rmtree(snap_dir, ignore_errors=True)
    if not is_png(final):
        return game["id"], False, f"output is not a valid PNG\n{tail}"
    return game["id"], True, "ok"


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

    env = dict(os.environ)
    env.setdefault("SDL_AUDIODRIVER", "dummy")

    failures = 0
    print(f"running {len(games)} game(s) for {manifest.get('emulator')}", flush=True)
    for game in games:
        gid, ok, detail = run_game(cfg, env, game)
        if ok:
            print(f"[{gid}] OK", flush=True)
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
